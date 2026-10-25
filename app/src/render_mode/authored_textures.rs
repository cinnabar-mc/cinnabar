//! Optional authored maps load once off the frame thread, after Enhanced is selected.

use std::{
    io,
    sync::{Arc, Mutex, Weak, mpsc},
};

use assets::texture::EnhancedTextureAssets;
use assets::{RuntimeAssets, material_keys::MaterialKeys};
use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;
use render_model::ENHANCED_RENDERING_ENABLED;
use render_scene::{ChunkTextureAssets, ChunkTextureReload};
use ui::RenderMode;

use client_state::RuntimeSettings;

mod enhanced_textures;

type AuthoredMaps = Option<Arc<EnhancedTextureAssets>>;
type Receiver = mpsc::Receiver<AuthoredMaps>;

enum Loading {
    Unrequested,
    Pending(Mutex<Receiver>),
    Complete(AuthoredMaps),
}

#[derive(Resource)]
pub(crate) struct AuthoredTextureLoading {
    base: Arc<RuntimeAssets>,
    keys: Option<MaterialKeys>,
    loading: Loading,
    /// The last local and server maps combined, and the result.
    combined: Option<CombinedMaps>,
}

/// One combination of local and server maps. The weak handles keep their allocations, so a
/// later carrier's maps never reuse an address the cache compares against.
struct CombinedMaps {
    local: Option<Weak<EnhancedTextureAssets>>,
    server: Weak<EnhancedTextureAssets>,
    maps: AuthoredMaps,
}

impl CombinedMaps {
    /// Whether this combination was made from exactly `local` and `server`.
    fn combines(
        &self,
        local: Option<&Arc<EnhancedTextureAssets>>,
        server: &Arc<EnhancedTextureAssets>,
    ) -> bool {
        let local = match (&self.local, local) {
            (None, None) => true,
            (Some(cached), Some(local)) => std::ptr::eq(cached.as_ptr(), Arc::as_ptr(local)),
            _ => false,
        };
        local && std::ptr::eq(self.server.as_ptr(), Arc::as_ptr(server))
    }
}

impl AuthoredTextureLoading {
    /// Retains the carrier and its keys without starting optional pack work.
    pub(crate) fn new(base: Arc<RuntimeAssets>, keys: Option<MaterialKeys>) -> Self {
        Self {
            base,
            keys,
            loading: Loading::Unrequested,
            combined: None,
        }
    }

    /// Starts one decoder only when Enhanced is enabled and explicitly selected.
    fn begin(
        &mut self,
        mode: RenderMode,
        spawn: impl FnOnce(Arc<RuntimeAssets>, MaterialKeys) -> io::Result<Receiver>,
    ) {
        if !ENHANCED_RENDERING_ENABLED
            || mode != RenderMode::Enhanced
            || !matches!(self.loading, Loading::Unrequested)
        {
            return;
        }
        let Some(keys) = self.keys.take() else {
            self.loading = Loading::Complete(None);
            return;
        };
        self.loading = match spawn(Arc::clone(&self.base), keys) {
            Ok(receiver) => Loading::Pending(Mutex::new(receiver)),
            Err(error) => {
                warn!(?error, "authored Enhanced texture worker could not start");
                Loading::Complete(None)
            }
        };
    }

    /// Accepts a completed worker result without blocking the frame thread.
    fn poll(&mut self) {
        let Loading::Pending(receiver) = &self.loading else {
            return;
        };
        let result = receiver
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .try_recv();
        match result {
            Ok(maps) => self.loading = Loading::Complete(maps),
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                warn!("authored Enhanced texture worker stopped; retained carrier materials");
                self.loading = Loading::Complete(None);
            }
        }
    }

    /// Returns completed maps only for the Enhanced selection.
    fn selected(&self, mode: RenderMode) -> AuthoredMaps {
        if mode == RenderMode::Enhanced
            && let Loading::Complete(maps) = &self.loading
        {
            maps.clone()
        } else {
            None
        }
    }

    /// Drops unsupported cached maps so future server carriers cannot inherit them.
    fn discard_unsupported(&mut self, limits: &wgpu::Limits) {
        if let Loading::Complete(Some(maps)) = &self.loading
            && !render_scene::enhanced_maps_fit_device_limits(maps, limits)
        {
            warn!("unsupported authored Enhanced maps; retained carrier materials");
            self.loading = Loading::Complete(None);
        }
    }
}

/// Loads and validates optional maps before publishing them to the frame thread.
fn spawn(
    base: Arc<RuntimeAssets>,
    keys: MaterialKeys,
    limits: Option<wgpu::Limits>,
) -> io::Result<Receiver> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("Enhanced terrain materials".to_owned())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let maps =
                enhanced_textures::load_optional_enhanced_textures(&base, &keys).filter(|maps| {
                    let supported = limits.as_ref().is_none_or(|limits| {
                        render_scene::enhanced_maps_fit_device_limits(maps, limits)
                    });
                    if !supported {
                        warn!("unsupported authored Enhanced maps; retained carrier materials");
                    }
                    supported
                });
            let _ = sender.send(maps);
        })?;
    Ok(receiver)
}

/// Combines the selected local maps with the session carrier's server texture-set maps, which
/// take precedence where both author a slot. Without the Enhanced selection there are no local
/// maps and server maps are dropped too, leaving every server texture its colour. Combined maps
/// the device cannot hold keep only the local ones.
fn with_server_maps(
    loading: &mut AuthoredTextureLoading,
    textures: &ChunkTextureAssets,
    mode: RenderMode,
    limits: Option<&wgpu::Limits>,
) -> AuthoredMaps {
    let local = loading.selected(mode);
    let server = textures.assets().server_enhanced();
    let (true, Some(server)) = (mode == RenderMode::Enhanced, server) else {
        // Nothing combined stays alive once these inputs no longer draw.
        loading.combined = None;
        return local;
    };
    if let Some(cached) = &loading.combined
        && cached.combines(local.as_ref(), server)
    {
        return cached.maps.clone();
    }
    let local_handle = local.as_ref().map(Arc::downgrade);
    let combined = EnhancedTextureAssets::merged(local.as_deref(), server)
        .filter(|maps| {
            limits.is_none_or(|limits| render_scene::enhanced_maps_fit_device_limits(maps, limits))
        })
        .map(Arc::new)
        .or_else(|| {
            warn!("unsupported server Enhanced maps; retained their colour");
            local
        });
    loading.combined = Some(CombinedMaps {
        local: local_handle,
        server: Arc::downgrade(server),
        maps: combined.clone(),
    });
    combined
}

/// Replaces optional maps only when their shared source changes.
fn apply(textures: &mut ChunkTextureAssets, maps: AuthoredMaps) {
    let unchanged = match (textures.enhanced(), maps.as_ref()) {
        (Some(current), Some(next)) => Arc::ptr_eq(current, next),
        (None, None) => true,
        _ => false,
    };
    if !unchanged {
        *textures = textures.with_updated_enhanced(maps);
    }
}

/// Polls optional loading and preserves carrier ownership during reloads.
pub(super) fn update_authored_textures(
    settings: Res<RuntimeSettings>,
    loading: Option<ResMut<AuthoredTextureLoading>>,
    textures: Option<ResMut<ChunkTextureAssets>>,
    reload: Option<Res<ChunkTextureReload>>,
    device: Option<Res<RenderDevice>>,
) {
    let (Some(mut loading), Some(mut textures)) = (loading, textures) else {
        return;
    };
    // A Vanilla fallback on an unsupported device never decodes or attaches authored maps.
    let mode = if ENHANCED_RENDERING_ENABLED {
        settings.user_settings_update().1.video.render_mode
    } else {
        RenderMode::Vanilla
    };
    let limits = device.as_ref().map(|device| device.limits());
    loading.begin(mode, |base, keys| spawn(base, keys, limits.clone()));
    loading.poll();
    if let Some(limits) = &limits {
        loading.discard_unsupported(limits);
    }
    if reload.is_some_and(|reload| reload.geometry_pending()) {
        return;
    }
    let maps = with_server_maps(&mut loading, &textures, mode, limits.as_ref());
    apply(&mut textures, maps);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds minimal valid authored pages for loader tests.
    fn maps() -> Arc<EnhancedTextureAssets> {
        let array = || assets::texture::TextureArray {
            layers: 1,
            mips: vec![assets::texture::TextureMip {
                size: 1,
                rgba8: vec![128, 128, 255, 255].into_boxed_slice(),
            }]
            .into_boxed_slice(),
        };
        Arc::new(
            EnhancedTextureAssets::new(
                [array(), array()],
                [array(), array()],
                [array(), array()],
                vec![u32::MAX; assets::MAX_TEXTURE_PAGES * assets::MAX_TEXTURE_LAYERS]
                    .into_boxed_slice(),
            )
            .unwrap(),
        )
    }

    /// A carrier whose overlay holds one texture with server Enhanced maps on its slot.
    fn server_carrier() -> Arc<RuntimeAssets> {
        let shard = assets::texture::TextureArray {
            layers: 1,
            mips: (0..5)
                .map(|level| assets::texture::TextureMip {
                    size: 16 >> level,
                    rgba8: vec![200; 4 * (16usize >> level).pow(2)].into_boxed_slice(),
                })
                .collect(),
        };
        let server = maps();
        let mut references = server.texture_refs.to_vec();
        references[assets::MAX_TEXTURE_LAYERS] = 1 << 31;
        let server = EnhancedTextureAssets::new(
            server.color_pages.clone(),
            server.normal_pages.clone(),
            server.mer_pages.clone(),
            references.into_boxed_slice(),
        )
        .unwrap();
        let overlay = assets::BlockOverlay {
            textures: vec![shard],
            enhanced: Some(Arc::new(server)),
            ..Default::default()
        };
        Arc::new(
            RuntimeAssets::diagnostic()
                .with_block_overlay(1, &overlay)
                .unwrap(),
        )
    }

    #[test]
    fn server_maps_attach_only_with_the_enhanced_selection_and_are_combined_once() {
        let carrier = server_carrier();
        let textures = ChunkTextureAssets::new(Arc::clone(&carrier));
        let mut loading = AuthoredTextureLoading::new(Arc::clone(&carrier), None);
        assert!(with_server_maps(&mut loading, &textures, RenderMode::Vanilla, None).is_none());
        let server = carrier.server_enhanced().unwrap();
        let alone = with_server_maps(&mut loading, &textures, RenderMode::Enhanced, None).unwrap();
        assert_eq!(alone.as_ref(), server.as_ref());
        let again = with_server_maps(&mut loading, &textures, RenderMode::Enhanced, None).unwrap();
        assert!(
            Arc::ptr_eq(&alone, &again),
            "unchanged inputs are not combined again"
        );
        loading.loading = Loading::Complete(Some(maps()));
        let combined =
            with_server_maps(&mut loading, &textures, RenderMode::Enhanced, None).unwrap();
        let slot = assets::TextureRef::new(1, 0).unwrap();
        assert_eq!(combined.material_reference(0, slot), 1 << 31);
        let tiny = wgpu::Limits {
            max_texture_dimension_2d: 0,
            ..Default::default()
        };
        loading.combined = None;
        let local = with_server_maps(&mut loading, &textures, RenderMode::Enhanced, Some(&tiny));
        assert!(
            local.is_some_and(|maps| maps.material_reference(0, slot) == u32::MAX),
            "maps the device cannot hold keep only the local ones"
        );
    }

    #[test]
    fn leaving_enhanced_releases_combined_maps_and_returning_combines_afresh() {
        let carrier = server_carrier();
        let textures = ChunkTextureAssets::new(Arc::clone(&carrier));
        let mut loading = AuthoredTextureLoading::new(Arc::clone(&carrier), None);
        let combined =
            with_server_maps(&mut loading, &textures, RenderMode::Enhanced, None).unwrap();
        assert!(with_server_maps(&mut loading, &textures, RenderMode::Vanilla, None).is_none());
        assert_eq!(
            Arc::strong_count(&combined),
            1,
            "nothing keeps maps combined for a selection that no longer draws them"
        );
        let again = with_server_maps(&mut loading, &textures, RenderMode::Enhanced, None).unwrap();
        assert!(!Arc::ptr_eq(&again, &combined));
        assert_eq!(again.as_ref(), combined.as_ref());
    }

    #[test]
    fn an_unchanged_reload_keeps_the_uploaded_maps_and_a_changed_pack_retires_them() {
        let carrier = server_carrier();
        let overlay = assets::BlockOverlay {
            textures: vec![assets::texture::TextureArray {
                layers: 1,
                mips: (0..5)
                    .map(|level| assets::texture::TextureMip {
                        size: 16 >> level,
                        rgba8: vec![200; 4 * (16usize >> level).pow(2)].into_boxed_slice(),
                    })
                    .collect(),
            }],
            enhanced: carrier.server_enhanced().cloned(),
            ..Default::default()
        };
        // A reload republishes the carrier around the same compiled overlay.
        let reloaded = Arc::new(
            RuntimeAssets::diagnostic()
                .with_block_overlay(1, &overlay)
                .unwrap(),
        );
        let mut loading = AuthoredTextureLoading::new(Arc::clone(&carrier), None);
        let mut textures = ChunkTextureAssets::new(Arc::clone(&carrier));
        let maps = with_server_maps(&mut loading, &textures, RenderMode::Enhanced, None);
        apply(&mut textures, maps);
        let retained = Arc::clone(textures.enhanced().unwrap());
        textures = ChunkTextureAssets::with_optional_enhanced(
            reloaded,
            textures.enhanced().cloned(),
            textures.revision,
        );
        let maps = with_server_maps(&mut loading, &textures, RenderMode::Enhanced, None);
        apply(&mut textures, maps);
        assert!(
            Arc::ptr_eq(textures.enhanced().unwrap(), &retained),
            "the same server maps are not combined or uploaded again"
        );
        // A different pack's carrier replaces the combined maps; nothing keeps the old ones.
        let changed = ChunkTextureAssets::new(server_carrier());
        let maps = with_server_maps(&mut loading, &changed, RenderMode::Enhanced, None);
        assert!(!Arc::ptr_eq(maps.as_ref().unwrap(), &retained));
        drop(textures);
        assert_eq!(
            Arc::strong_count(&retained),
            1,
            "the retired maps are released"
        );
    }

    #[test]
    fn vanilla_selection_never_starts_optional_texture_work() {
        let base = Arc::new(RuntimeAssets::diagnostic());
        let mut loading = AuthoredTextureLoading::new(base, Some(MaterialKeys::default()));
        loading.begin(RenderMode::Vanilla, |_, _| {
            panic!("vanilla must not inspect or load optional authored packs")
        });
        assert!(matches!(loading.loading, Loading::Unrequested));
        assert!(loading.selected(RenderMode::Vanilla).is_none());
    }

    /// Completed maps follow mode selection without restarting the decoder.
    #[test]
    fn completed_maps_follow_selection_without_decoding_again() {
        let base = Arc::new(RuntimeAssets::diagnostic());
        let authored = maps();
        let mut loading = AuthoredTextureLoading::new(base, None);
        loading.loading = Loading::Complete(Some(authored.clone()));
        assert!(Arc::ptr_eq(
            &loading.selected(RenderMode::Enhanced).unwrap(),
            &authored
        ));
        assert!(loading.selected(RenderMode::Vanilla).is_none());
        loading.begin(RenderMode::Enhanced, |_, _| {
            panic!("completed authored maps must not decode again")
        });
        assert!(loading.selected(RenderMode::Enhanced).is_some());
    }

    #[test]
    fn attaching_maps_preserves_current_server_assets_and_revision() {
        let server_runtime = Arc::new(RuntimeAssets::diagnostic());
        let mut textures = ChunkTextureAssets::with_revision(server_runtime.clone(), 17);
        let carrier_identity = textures.identity();
        let original_identity = textures.binding_identity();
        let authored = maps();
        apply(&mut textures, Some(authored.clone()));
        assert!(Arc::ptr_eq(textures.assets(), &server_runtime));
        assert!(Arc::ptr_eq(textures.enhanced().unwrap(), &authored));
        assert_eq!(textures.identity(), carrier_identity);
        let enhanced_identity = textures.binding_identity();
        assert_ne!(enhanced_identity, original_identity);
        apply(&mut textures, Some(authored));
        assert_eq!(textures.binding_identity(), enhanced_identity);
        apply(&mut textures, None);
        assert_eq!(textures.binding_identity(), original_identity);
    }

    /// The disabled renderer decodes and attaches nothing even with completed maps.
    #[test]
    fn disabled_enhanced_never_loads_or_attaches_authored_maps() {
        let base = Arc::new(RuntimeAssets::diagnostic());
        let mut settings = RuntimeSettings::default();
        crate::render_mode::set_render_mode(&mut settings, RenderMode::Enhanced);
        let mut app = App::new();
        app.insert_resource(settings)
            .insert_resource(AuthoredTextureLoading::new(
                base.clone(),
                Some(MaterialKeys::default()),
            ))
            .insert_resource(ChunkTextureAssets::new(base))
            .add_systems(Update, update_authored_textures);
        app.update();
        assert!(matches!(
            app.world().resource::<AuthoredTextureLoading>().loading,
            Loading::Unrequested,
        ));
        app.world_mut()
            .resource_mut::<AuthoredTextureLoading>()
            .loading = Loading::Complete(Some(maps()));
        app.update();
        assert!(
            app.world()
                .resource::<ChunkTextureAssets>()
                .enhanced()
                .is_none()
        );
    }

    #[test]
    fn unsupported_maps_are_discarded_before_server_assets_can_inherit_them() {
        let base = Arc::new(RuntimeAssets::diagnostic());
        let array = assets::texture::TextureArray {
            layers: 2,
            mips: Box::new([assets::texture::TextureMip {
                size: 1,
                rgba8: vec![128; 8].into_boxed_slice(),
            }]),
        };
        let authored = Arc::new(
            EnhancedTextureAssets::new(
                [array.clone(), array.clone()],
                [array.clone(), array.clone()],
                [array.clone(), array],
                vec![u32::MAX; assets::MAX_TEXTURE_PAGES * assets::MAX_TEXTURE_LAYERS]
                    .into_boxed_slice(),
            )
            .unwrap(),
        );
        let mut loading = AuthoredTextureLoading::new(base.clone(), None);
        loading.loading = Loading::Complete(Some(authored));
        loading.discard_unsupported(&wgpu::Limits {
            max_texture_array_layers: 1,
            ..Default::default()
        });
        assert!(matches!(loading.loading, Loading::Complete(None)));
        assert!(loading.selected(RenderMode::Enhanced).is_none());
    }
}
