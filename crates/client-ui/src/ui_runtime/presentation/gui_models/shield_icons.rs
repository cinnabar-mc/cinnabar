//! Stack-specific shield artwork shares the startup model and active pack pattern textures.
use super::*;
use std::sync::{Mutex, OnceLock};

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
    hands: BTreeMap<Key, assets::EquipmentTexture>,
    source_limit: u32,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    appearance: render_api::ShieldPattern,
    source: usize,
    size: [u16; 2],
}

impl Key {
    /// Identifies a composition by its cloth data and retained base allocation.
    fn new(appearance: render_api::ShieldPattern, base: &assets::EquipmentTexture) -> Self {
        Self {
            appearance,
            source: base.rgba8.as_ptr() as usize,
            size: [base.width, base.height],
        }
    }
}

#[derive(Clone)]
struct Image {
    icon: IconRef,
    texture: IconRef,
    sprite: Arc<Sprite>,
    /// Keeps the source pointer used by the key alive until this atlas is retired.
    _source: Arc<[u8]>,
}

struct Sprite {
    pixels: assets::IconSprite,
    vertices: OnceLock<Option<Arc<[render_model::ActorRigVertex]>>>,
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
        let key = Key::new(appearance, base);
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
            sprite: Arc::new(Sprite {
                pixels: thumbnail,
                vertices: OnceLock::new(),
            }),
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

    /// Shares the decorated inventory sprite and its lazily extruded alpha edges with previews.
    pub(super) fn shield_preview_sprite(
        &self,
        appearance: &render_api::ShieldPattern,
    ) -> Option<(IconRef, Arc<[render_model::ActorRigVertex]>)> {
        let image = self.shield_image(appearance, None)?;
        let vertices = image
            .sprite
            .vertices
            .get_or_init(|| {
                let pixels = &image.sprite.pixels;
                render_model::held_sprite_vertices(
                    usize::from(pixels.width),
                    usize::from(pixels.height),
                    &pixels.rgba8,
                    [0.0, 0.0, 1.0, 1.0],
                )
                .map(Into::into)
            })
            .clone()?;
        Some((image.icon, vertices))
    }

    /// Retains the same decorated pixels for the compatibility preview's held-item raster.
    pub(in crate::ui_runtime::presentation) fn shield_preview_texture(
        &self,
        appearance: &render_api::ShieldPattern,
    ) -> Option<player_preview::PreviewTexture> {
        let image = self.shield_image(appearance, None)?;
        let pixels = &image.sprite.pixels;
        Some(player_preview::PreviewTexture {
            rgba: Arc::clone(&pixels.rgba8),
            width: pixels.width,
            height: pixels.height,
            tint: None,
        })
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
        let hands = self
            .gui_models
            .shield_icons
            .cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .hands
            .clone();
        self.prepare_shield_images(appearances, hands);
    }

    /// Fits the currently selected hand sheets together with the visible inventory art.
    pub(in crate::ui_runtime::presentation) fn prepare_shield_hands<'a>(
        &mut self,
        hands: impl IntoIterator<Item = (&'a render_api::ShieldPattern, &'a assets::EquipmentTexture)>,
    ) {
        let hands = hands
            .into_iter()
            .map(|(pattern, base)| (Key::new(pattern.clone(), base), base.clone()))
            .collect();
        let appearances = self
            .gui_models
            .shield_icons
            .cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .wanted
            .clone();
        self.prepare_shield_images(appearances, hands);
    }

    /// Rebuilds every demanded composition at one resolution before consumers retain addresses.
    fn prepare_shield_images(
        &mut self,
        appearances: BTreeSet<render_api::ShieldPattern>,
        hands: BTreeMap<Key, assets::EquipmentTexture>,
    ) {
        let Some(source) = self.gui_models.sources.shield.as_ref() else {
            return;
        };
        let Some(startup) = self.equipment_catalog.as_deref() else {
            return;
        };
        let active = self.gui_models.pack_equipment.source.as_deref();
        let base = active
            .and_then(|catalog| catalog.texture(&source.texture.identifier))
            .unwrap_or(&source.texture);
        let mut cache = self
            .gui_models
            .shield_icons
            .cache
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if cache.wanted != appearances || !cache.hands.keys().eq(hands.keys()) {
            if let Some(inventory_cache) = self.inventory_cache.as_mut() {
                inventory_cache.invalidate_icons();
            }
            let mut limit = render_model::UI_MODEL_ATLAS_SIDE;
            loop {
                *cache = Cache {
                    wanted: appearances.clone(),
                    hands: hands.clone(),
                    source_limit: limit,
                    dirty: true,
                    ..Default::default()
                };
                let complete = appearances
                    .iter()
                    .map(|appearance| (appearance, base))
                    .chain(hands.iter().map(|(key, base)| (&key.appearance, base)))
                    .all(|(appearance, base)| {
                        cache
                            .insert(
                                appearance.clone(),
                                source,
                                base,
                                (self.textures.dynamic_start()
                                    + render_model::UI_SHIELD_PAGE_OFFSET)
                                    as u16,
                                |path| {
                                    active
                                        .and_then(|catalog| catalog.texture(path))
                                        .or_else(|| startup.texture(path))
                                },
                            )
                            .is_some()
                    });
                if complete || limit <= 1 {
                    break;
                }
                limit /= 2;
            }
        }
        drop(cache);
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
