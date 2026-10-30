//! Borrowed artwork resolution for the bounded world item model cache.
use super::*;
use render_model::{DroppedItemModel, dropped_item::ItemModelCost};
use std::hash::{Hash, Hasher};

/// Immutable shield sources; pointer identities remain valid while a cache key retains them.
#[derive(Clone)]
pub struct WorldShieldArtwork {
    source: Arc<shield_icons::Source>,
    startup: Arc<assets::RuntimeEquipmentCatalog>,
    active: Option<Arc<assets::RuntimeEquipmentCatalog>>,
    appearance: Option<Arc<render_api::ShieldPattern>>,
    max_pixels: usize,
}

impl PartialEq for WorldShieldArtwork {
    /// Compares immutable source ownership without hashing or comparing raster bytes.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.source, &other.source)
            && Arc::ptr_eq(&self.startup, &other.startup)
            && self.active.as_ref().map(Arc::as_ptr) == other.active.as_ref().map(Arc::as_ptr)
            && self.appearance == other.appearance
            && self.max_pixels == other.max_pixels
    }
}
impl Eq for WorldShieldArtwork {}
impl Hash for WorldShieldArtwork {
    /// Hashes the same retained source addresses and appearance used by equality.
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.source).hash(state);
        Arc::as_ptr(&self.startup).hash(state);
        self.active.as_ref().map(Arc::as_ptr).hash(state);
        self.appearance.hash(state);
        self.max_pixels.hash(state);
    }
}

impl WorldShieldArtwork {
    /// Looks up a mask through the active pack, then the fetched startup carrier.
    fn texture(&self, path: &str) -> Option<&assets::EquipmentTexture> {
        self.active
            .as_ref()
            .and_then(|active| active.texture(path))
            .or_else(|| self.startup.texture(path))
    }
    /// Selects the active replacement for the static shield's original base sheet.
    fn base(&self) -> &assets::EquipmentTexture {
        self.texture(&self.source.texture.identifier)
            .unwrap_or(&self.source.texture)
    }
    /// Measures the same normalized composition extent used when constructing the model.
    fn dimensions(&self) -> Option<[u16; 2]> {
        let [mut width, mut height] = if let Some(appearance) = &self.appearance {
            assets::item::shield::composition_size(
                self.base(),
                appearance.base,
                appearance
                    .layers
                    .iter()
                    .map(|layer| (layer.pattern.as_ref(), layer.color)),
                |path| self.texture(path),
            )?
        } else {
            [self.base().width, self.base().height]
        };
        if self.appearance.is_some() {
            while usize::from(width) * usize::from(height) > self.max_pixels {
                width = (width / 2).max(1);
                height = (height / 2).max(1);
            }
        }
        Some([width, height])
    }
    /// Measures original geometry and admitted source pixels before cache allocation.
    pub fn cost(&self) -> Option<ItemModelCost> {
        let [width, height] = self.dimensions()?;
        ItemModelCost::new(
            u32::from(width.max(height)),
            1,
            usize::from(width) * usize::from(height) * 4,
            render_model::equipment::shield::vertex_count(&self.source.geometry)?,
        )
    }
    /// Keeps decorated geometry while reducing its sheet for another bounded admission attempt.
    pub fn reduced(&self) -> Option<Self> {
        self.appearance.as_ref()?;
        let [width, height] = self.dimensions()?;
        if width == 1 && height == 1 {
            return None;
        }
        Some(Self {
            max_pixels: usize::from((width / 2).max(1)) * usize::from((height / 2).max(1)),
            ..self.clone()
        })
    }
    /// Composes a decorated sheet only after the bounded model cache admits this source.
    pub fn model(&self) -> Option<DroppedItemModel> {
        let texture = if let Some(appearance) = &self.appearance {
            assets::item::shield::compose(
                self.base(),
                appearance.base,
                appearance
                    .layers
                    .iter()
                    .map(|layer| (layer.pattern.as_ref(), layer.color)),
                self.max_pixels,
                |path| self.texture(path),
            )?
        } else {
            self.base().clone()
        };
        render_model::equipment::shield::model(&self.source.geometry, texture)
            .map(DroppedItemModel::Block)
    }
}

impl UiPresentationRuntime {
    /// Resolves the static shield model and current pack masks without allocating artwork.
    pub fn world_shield_artwork(
        &self,
        appearance: Option<&Arc<render_api::ShieldPattern>>,
    ) -> Option<WorldShieldArtwork> {
        Some(WorldShieldArtwork {
            source: Arc::clone(self.gui_models.sources.shield.as_ref()?),
            startup: Arc::clone(self.equipment_catalog.as_ref()?),
            active: self.gui_models.pack_equipment.source.clone(),
            appearance: appearance.cloned(),
            max_pixels: assets::texture::policy::MAX_SUBSCRIBER_BYTES / 4,
        })
    }
}
