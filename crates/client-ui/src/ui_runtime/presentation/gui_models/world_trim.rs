//! Trimmed armor sprites for world item drawers, which use the same per-stack trimmed icon as
//! the inventory.
use super::*;
use crate::ui_runtime::presentation::item_sprite::ItemSpritePixels;
use render_model::{DroppedItemModel, DroppedItemSprite, dropped_item::ItemModelCost};
use std::hash::{Hash, Hasher};

/// One stack's trim over its selected icon. Retaining the catalogs keeps their pointer
/// identities valid while a cache key holds this value.
#[derive(Clone)]
pub struct WorldTrimArtwork {
    icon: IconRef,
    startup: Arc<assets::RuntimeEquipmentCatalog>,
    active: Option<Arc<assets::RuntimeEquipmentCatalog>>,
    identifier: Arc<str>,
    trim: render_api::ItemTrim,
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
            && self.trim == other.trim
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
        self.trim.hash(state);
        self.native.hash(state);
    }
}

impl WorldTrimArtwork {
    /// Composes the trimmed sprite within `max_side`, keeping the icon's crop and reduction.
    pub fn sprite(&self, icons: &UiPresentationRuntime, max_side: u32) -> Option<ItemSpritePixels> {
        let base = icons.resolved_item_sprite(self.icon, max_side)?;
        let base = assets::EquipmentTexture {
            identifier: self.identifier.as_ref().into(),
            width: u16::try_from(base.width).ok()?,
            height: u16::try_from(base.height).ok()?,
            rgba8: base.rgba8.into(),
        };
        let binding = self
            .active
            .as_deref()
            .and_then(|catalog| catalog.binding(&self.identifier))
            .or_else(|| self.startup.binding(&self.identifier));
        let composed = trim_icons::compose_trim(
            &self.startup,
            self.active.as_deref(),
            &base,
            binding,
            &self.trim,
            &self.identifier,
            true,
            None,
        )?;
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

    /// Measures the composed sprite with the alpha rule used when it is extruded.
    pub fn cost(&self, icons: &UiPresentationRuntime, max_side: u32) -> Option<ItemModelCost> {
        let sprite = self.sprite(icons, max_side)?;
        let threshold = if self.native { 2 } else { 1 };
        let vertices = render_model::sampled_sprite_vertex_count(
            sprite.width as usize,
            sprite.height as usize,
            |x, y| sprite.rgba8[(y * sprite.width as usize + x) * 4 + 3] >= threshold,
        )?;
        ItemModelCost::new(
            sprite.width.max(sprite.height),
            1,
            sprite.rgba8.len(),
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
        identifier: &str,
        icon: IconRef,
        trim: Option<&render_api::ItemTrim>,
        native: bool,
    ) -> Option<WorldTrimArtwork> {
        let trim = trim?;
        assets::item::trim::armor_slot(identifier)?;
        Some(WorldTrimArtwork {
            icon,
            startup: Arc::clone(self.equipment_catalog.as_ref()?),
            active: self.gui_models.pack_equipment.source.clone(),
            identifier: identifier.into(),
            trim: trim.clone(),
            native,
        })
    }
}
