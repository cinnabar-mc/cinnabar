//! Source facts retained alongside the immutable world carrier.

use serde::{Deserialize, Serialize};

/// The terrain state and raster from which one material was compiled.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialTextureSource {
    pub key: Box<str>,
    pub variant: u32,
    pub path: Box<str>,
    /// Resolved image filename relative to the source pack, including its extension.
    pub raster: Box<str>,
    /// Atlas multiplier already applied to this material's carrier pixels.
    pub baked_tint: Option<[u8; 3]>,
}

/// A block's binding identity and face orientation without a geometry override.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlockTextureSource {
    pub visual: u32,
    pub name: Box<str>,
    pub binding_name: Box<str>,
    pub source_faces: [u8; 6],
    /// Quarter-turn UV rotation for each world face when the binding is per-face.
    pub source_rotations: [bool; 6],
    /// Both world faces and carried art may be rebound only for admitted full cubes.
    pub cube_binding_eligible: bool,
    pub carried_keys: [Option<Box<str>>; 6],
    pub carried_fallback_keys: [Option<Box<str>>; 6],
}

/// Largest number of state variants or positional choices in a terrain entry.
pub const MAX_TERRAIN_VARIANTS: usize = 256;

/// The largest raw terrain choice weight before normalization.
const MAX_TERRAIN_WEIGHT: f32 = 1_000_000.0;

/// Clamps raw terrain weights before normalizing the nonempty choice set.
pub fn normalize_terrain_weights(weights: &mut [f32]) -> bool {
    if weights.is_empty() || weights.iter().any(|weight| weight.is_nan()) {
        return false;
    }
    for weight in weights.iter_mut() {
        *weight = weight.clamp(0.05, MAX_TERRAIN_WEIGHT);
    }
    let total: f32 = weights.iter().sum();
    for weight in weights {
        *weight /= total;
    }
    true
}

impl super::MaterialKeys {
    /// Retains reload source facts, with block bindings ordered by sequential visual ID.
    #[must_use]
    pub fn with_sources(
        mut self,
        terrain: impl IntoIterator<Item = (Box<str>, serde_json::Value)>,
        materials: impl IntoIterator<Item = (u32, MaterialTextureSource)>,
        blocks: impl IntoIterator<Item = BlockTextureSource>,
    ) -> Self {
        self.terrain = terrain.into_iter().collect();
        self.material_sources = materials.into_iter().collect();
        self.block_sources = blocks.into_iter().collect();
        self.block_sources
            .sort_unstable_by_key(|source| source.visual);
        self
    }

    /// Complete base terrain entries, including state arrays and weighted choices.
    pub fn terrain_entries(&self) -> impl Iterator<Item = (&str, &serde_json::Value)> {
        self.terrain
            .iter()
            .map(|(key, entry)| (key.as_ref(), entry))
    }

    /// Source path and state variant of an individual compiled material.
    pub fn material_source(&self, material: u32) -> Option<&MaterialTextureSource> {
        self.material_sources.get(&material)
    }

    /// All material source routes, including independent weighted leaf copies.
    pub fn material_sources(&self) -> impl Iterator<Item = (u32, &MaterialTextureSource)> {
        self.material_sources
            .iter()
            .map(|(&id, source)| (id, source))
    }

    /// Records the pixel size of each source image, keyed by its `raster` filename.
    #[must_use]
    pub fn with_raster_sizes(
        mut self,
        sizes: impl IntoIterator<Item = (Box<str>, [u32; 2])>,
    ) -> Self {
        self.raster_sizes = sizes.into_iter().collect();
        self
    }

    /// Pixel width and height of a source image, by its `raster` filename.
    pub fn raster_size(&self, raster: &str) -> Option<[u32; 2]> {
        self.raster_sizes.get(raster).copied()
    }

    /// Materials whose carrier layer holds their source image unchanged: an untinted image of
    /// exactly one tile, which compilation neither downsamples nor reduces to an animation frame.
    pub fn exact_carrier_rasters(&self) -> impl Iterator<Item = (u32, &MaterialTextureSource)> {
        self.material_sources().filter(|(_, source)| {
            source.baked_tint.is_none()
                && self.raster_size(&source.raster) == Some([crate::TILE_SIZE; 2])
        })
    }

    /// Per-block binding identities, in the carrier's sequential visual order.
    pub fn block_sources(&self) -> &[BlockTextureSource] {
        &self.block_sources
    }
}
