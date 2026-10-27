//! Stack-specific shield artwork shares the startup model and active pack pattern textures.
use super::*;
use std::sync::Mutex;

/// The ownerless GUI shield definition admitted during source preparation.
pub(in crate::ui_runtime::presentation) struct Source {
    pub(super) geometry: assets::EntityGeometry,
    pub(super) texture: assets::EquipmentTexture,
}

#[derive(Default)]
pub(in crate::ui_runtime::presentation) struct ShieldIcons {
    cache: Mutex<Cache>,
}

#[derive(Default)]
struct Cache {
    atlas: Option<atlas::Atlas>,
    icons: BTreeMap<Key, Image>,
    models: BTreeMap<IconKey, Arc<UiMesh>>,
    pages: Vec<UiTexturePage>,
    dirty: bool,
    wanted: BTreeSet<render_api::ShieldPattern>,
    source_limit: u32,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    appearance: render_api::ShieldPattern,
    source: usize,
    size: [u16; 2],
}

#[derive(Clone)]
struct Image {
    icon: IconRef,
    texture: IconRef,
    /// Keeps the source pointer used by the key alive until this atlas is retired.
    _source: Arc<[u8]>,
}

impl Cache {
    /// Prepares one immutable appearance without depending on count, stack ID or blocking tick.
    fn insert<'a>(
        &mut self,
        appearance: render_api::ShieldPattern,
        source: &'a Source,
        base: &'a assets::EquipmentTexture,
        first: u16,
        texture: impl FnMut(&str) -> Option<&'a assets::EquipmentTexture>,
    ) -> Option<Image> {
        let key = Key {
            appearance,
            source: base.rgba8.as_ptr() as usize,
            size: [base.width, base.height],
        };
        if let Some(image) = self.icons.get(&key) {
            return Some(image.clone());
        }
        let limit = if self.source_limit == 0 {
            render_model::UI_MODEL_ATLAS_SIDE
        } else {
            self.source_limit
        };
        let mut composed = assets::item::shield::compose(
            base,
            key.appearance.base,
            key.appearance
                .layers
                .iter()
                .map(|layer| (layer.pattern.as_ref(), layer.color)),
            (limit * limit) as usize,
            texture,
        )?;
        let mut thumbnail = assets::item::shield::bake_icon(&source.geometry, &composed)?;
        if let Some((size, pixels)) =
            render_model::fit_rgba_within(composed.width, composed.height, &composed.rgba8, limit)
        {
            [composed.width, composed.height] = size;
            composed.rgba8 = pixels.into();
        }
        if let Some((size, pixels)) = render_model::fit_rgba_within(
            thumbnail.width,
            thumbnail.height,
            &thumbnail.rgba8,
            limit,
        ) {
            [thumbnail.width, thumbnail.height] = size;
            thumbnail.rgba8 = pixels.into();
        }
        let atlas = self
            .atlas
            .get_or_insert_with(|| atlas::Atlas::new(first, render_model::MAX_UI_SHIELD_PAGES));
        let (image, icon) = atlas.try_insert(
            [
                ([composed.width, composed.height], composed.rgba8.as_ref()),
                (
                    [thumbnail.width, thumbnail.height],
                    thumbnail.rgba8.as_ref(),
                ),
            ],
            |icons| Some((icons[0], icons[1])),
        )?;
        let mesh = item_gui::shield(&source.geometry, &composed, image)?;
        self.models.insert(icon_key(icon), mesh);
        let result = Image {
            icon,
            texture: image,
            _source: Arc::clone(&base.rgba8),
        };
        self.icons.insert(key, result.clone());
        self.dirty = true;
        Some(result)
    }
}

impl UiPresentationRuntime {
    /// Resolves a decorated shield after its stack's identifier has been admitted.
    pub(in crate::ui_runtime::presentation) fn shield_icon(
        &self,
        appearance: &render_api::ShieldPattern,
    ) -> Option<IconRef> {
        self.shield_image(appearance, None).map(|image| image.icon)
    }

    /// Uses the selected hand model's base sheet while sharing its banner overlays.
    pub(in crate::ui_runtime::presentation) fn shield_texture(
        &self,
        appearance: &render_api::ShieldPattern,
        base: &assets::EquipmentTexture,
    ) -> Option<IconRef> {
        self.shield_image(appearance, Some(base))
            .map(|image| image.texture)
    }

    /// Shares one composition between inventory art and hand models using the same sheet.
    fn shield_image(
        &self,
        appearance: &render_api::ShieldPattern,
        base: Option<&assets::EquipmentTexture>,
    ) -> Option<Image> {
        let source = self.gui_models.sources.shield.as_ref()?;
        let startup = self.equipment_catalog.as_deref()?;
        let active = self.gui_models.pack_equipment.source.as_deref();
        let base = base.unwrap_or_else(|| {
            active
                .and_then(|catalog| catalog.texture(&source.texture.identifier))
                .unwrap_or(&source.texture)
        });
        let mut cache = self
            .gui_models
            .shield_icons
            .cache
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        cache.insert(
            appearance.clone(),
            source,
            base,
            (self.textures.dynamic_start() + render_model::UI_SHIELD_PAGE_OFFSET) as u16,
            |path| {
                active
                    .and_then(|catalog| catalog.texture(path))
                    .or_else(|| startup.texture(path))
            },
        )
    }

    /// Reclaims unused stack art before resolving this frame, so old icon addresses cannot survive a repack.
    pub(in crate::ui_runtime::presentation) fn prepare_shield_icons(
        &mut self,
        appearances: impl IntoIterator<Item = Arc<render_api::ShieldPattern>>,
    ) {
        let appearances: BTreeSet<_> = appearances
            .into_iter()
            .map(|appearance| appearance.as_ref().clone())
            .collect();
        let reset = {
            let cache = self
                .gui_models
                .shield_icons
                .cache
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            cache.wanted != appearances
        };
        if reset {
            self.inventory_cache = Some(Default::default());
            let mut limit = render_model::UI_MODEL_ATLAS_SIDE;
            loop {
                *self
                    .gui_models
                    .shield_icons
                    .cache
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Cache {
                    wanted: appearances.clone(),
                    source_limit: limit,
                    dirty: true,
                    ..Default::default()
                };
                let complete = appearances
                    .iter()
                    .all(|appearance| self.shield_icon(appearance).is_some());
                if complete || limit <= 1 {
                    break;
                }
                limit /= 2;
            }
        }
        self.publish_shield_icons();
    }

    /// Publishes texels prepared during immutable item capture before the UI frame samples them.
    pub(in crate::ui_runtime::presentation) fn publish_shield_icons(&mut self) {
        let dirty = {
            let mut cache = self
                .gui_models
                .shield_icons
                .cache
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if cache.dirty {
                cache.pages = cache
                    .atlas
                    .as_ref()
                    .and_then(atlas::Atlas::pages)
                    .unwrap_or_default();
                cache.dirty = false;
                true
            } else {
                false
            }
        };
        if dirty {
            self.rebuild_dynamic_textures();
        }
    }
}

impl ShieldIcons {
    /// Discards addresses whose source catalog or session has changed.
    pub(in crate::ui_runtime::presentation) fn clear(&mut self) {
        *self = Self::default();
    }
    /// Copies shared immutable pages for the current texture generation.
    pub(in crate::ui_runtime::presentation) fn pages(&self) -> Vec<UiTexturePage> {
        self.cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pages
            .clone()
    }

    /// Replaces only this cache's icon cells with their full-resolution GUI geometry.
    pub(super) fn apply(&self, nodes: &mut [UiNode], modulated: &mut ModulatedMeshes) {
        let cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
        replace_model_icons(
            nodes,
            None,
            &cache.models.keys().copied().collect(),
            || None,
            |key| cache.models.get(key),
            |mesh, color, glint| modulated.get(mesh, color, glint),
        );
    }
}
