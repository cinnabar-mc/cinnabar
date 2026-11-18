//! Trimmed armor sprites for world item drawers, which use the same per-stack trimmed icon as
//! the inventory.
use super::*;
use crate::ui_runtime::presentation::item_sprite::{IconPixels, ItemSpritePixels};
use render_model::{DroppedItemModel, DroppedItemSprite, dropped_item::ItemModelCost};
use std::hash::{Hash, Hasher};

/// One stack's trim material over its selected icon. Icons use the slot's item mask, so the
/// pattern never changes their pixels and is not part of this identity. Retaining the catalogs
/// keeps their pointer identities valid while a cache key holds this value.
#[derive(Clone)]
pub struct WorldTrimArtwork {
    icon: IconRef,
    startup: Arc<assets::RuntimeEquipmentCatalog>,
    active: Option<Arc<assets::RuntimeEquipmentCatalog>>,
    identifier: Arc<str>,
    trim: render_api::ItemTrim,
    item: trim_icons::TrimItem,
    native: bool,
}

impl PartialEq for WorldTrimArtwork {
    /// Compares the selected icon cell and retained catalog ownership without comparing pixels.
    fn eq(&self, other: &Self) -> bool {
        self.icon.page == other.icon.page
            && self.icon.uv == other.icon.uv
            && Arc::ptr_eq(&self.startup, &other.startup)
            && self.active.as_ref().map(Arc::as_ptr) == other.active.as_ref().map(Arc::as_ptr)
            && self.identifier == other.identifier
            && self.trim.material == other.trim.material
            && self.item == other.item
            && self.native == other.native
    }
}
impl Eq for WorldTrimArtwork {}
impl Hash for WorldTrimArtwork {
    /// Hashes the same icon cell, catalog addresses and stack identity used by equality.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.icon.page.hash(state);
        self.icon.uv.hash(state);
        Arc::as_ptr(&self.startup).hash(state);
        self.active.as_ref().map(Arc::as_ptr).hash(state);
        self.identifier.hash(state);
        self.trim.material.hash(state);
        self.item.hash(state);
        self.native.hash(state);
    }
}

impl WorldTrimArtwork {
    /// The item mask and material palettes, with the active pack's aliases first.
    fn layers(&self) -> Option<assets::item::trim::TrimLayers<'_>> {
        let binding = self
            .active
            .as_deref()
            .and_then(|catalog| catalog.binding(&self.identifier))
            .or_else(|| self.startup.binding(&self.identifier));
        trim_icons::trim_layers(
            &self.startup,
            self.active.as_deref(),
            binding,
            &self.trim,
            &self.item,
            true,
        )
    }

    /// Composes the trimmed sprite within `max_side`, keeping the icon's crop and reduction.
    pub fn sprite(&self, icons: &UiPresentationRuntime, max_side: u32) -> Option<ItemSpritePixels> {
        let base = icons.resolved_item_sprite(self.icon, max_side)?;
        let base = assets::EquipmentTexture {
            identifier: self.identifier.as_ref().into(),
            width: u16::try_from(base.width).ok()?,
            height: u16::try_from(base.height).ok()?,
            rgba8: base.rgba8.into(),
        };
        let composed = self.layers()?.compose(&base, None)?;
        let ([width, height], rgba8) = render_model::fit_rgba_within(
            composed.width,
            composed.height,
            &composed.rgba8,
            max_side,
        )
        .unwrap_or(([composed.width, composed.height], composed.rgba8.to_vec()));
        Some(ItemSpritePixels {
            width: u32::from(width),
            height: u32::from(height),
            rgba8,
        })
    }

    /// Measures the sprite [`Self::sprite`] would build, with the alpha rule used when it is
    /// extruded, reading the atlas and trim mask in place instead of composing pixels.
    pub fn cost(&self, icons: &UiPresentationRuntime, max_side: u32) -> Option<ItemModelCost> {
        let base = IconPixels::new(icons, self.icon, max_side)?;
        let base_size = [
            u16::try_from(base.width).ok()?,
            u16::try_from(base.height).ok()?,
        ];
        let layers = self.layers()?;
        let [width, height] = layers.size(base_size);
        let fitted = render_model::fitted_dimensions(width, height, max_side).map(usize::from);
        let threshold = if self.native { 2 } else { 1 };
        // The vertex count visits each texel's neighbours, so each reduced alpha is read once.
        let mut opaque = Vec::with_capacity(fitted[0] * fitted[1]);
        for y in 0..fitted[1] {
            for x in 0..fitted[0] {
                let alpha = render_model::fitted_channel(width, height, max_side, x, y, |x, y| {
                    layers.alpha(
                        base_size,
                        |x, y| base.pixel(x as u32, y as u32)[3],
                        x as u16,
                        y as u16,
                    )
                });
                opaque.push(alpha >= threshold);
            }
        }
        let vertices = render_model::sampled_sprite_vertex_count(fitted[0], fitted[1], |x, y| {
            opaque[y * fitted[0] + x]
        })?;
        ItemModelCost::new(
            fitted[0].max(fitted[1]) as u32,
            1,
            fitted[0] * fitted[1] * 4,
            vertices,
        )
    }

    /// Builds the extruded sprite model only after the bounded model cache admits it.
    pub fn model(&self, icons: &UiPresentationRuntime, max_side: u32) -> Option<DroppedItemModel> {
        let sprite = self.sprite(icons, max_side)?;
        let sprite = DroppedItemSprite {
            width: sprite.width,
            height: sprite.height,
            rgba8: sprite.rgba8.into(),
        };
        Some(if self.native {
            DroppedItemModel::NativeSprite(sprite)
        } else {
            DroppedItemModel::Sprite(sprite)
        })
    }
}

impl UiPresentationRuntime {
    /// Selects a trimmed armor stack's world sprite over its resolved `icon`, as vanilla armor
    /// icons follow the stack's trim. `None` for untrimmed stacks and items that take no trim.
    pub fn world_trim_artwork(
        &self,
        identifier: &Arc<str>,
        icon: IconRef,
        trim: Option<&render_api::ItemTrim>,
        native: bool,
    ) -> Option<WorldTrimArtwork> {
        let trim = trim?;
        let item = self.gui_models.trim_icons.item(identifier)?;
        Some(WorldTrimArtwork {
            icon,
            startup: Arc::clone(self.equipment_catalog.as_ref()?),
            active: self.gui_models.pack_equipment.source.clone(),
            identifier: Arc::clone(identifier),
            // Icons use the slot's item mask, so the pattern never changes their pixels.
            trim: render_api::ItemTrim {
                material: Arc::clone(&trim.material),
                pattern: "".into(),
            },
            item,
            native,
        })
    }
}
