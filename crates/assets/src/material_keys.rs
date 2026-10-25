//! Terrain texture key index over a compiled carrier's materials, so a session
//! can retexture vanilla blocks from a server pack's `terrain_texture.json`.
//! It also carries the vanilla key-to-image aliases, since installs keep no unpacked pack.

use std::collections::BTreeMap;

use crate::texture::flipbook::FlipbookSource;
mod wire;

mod sources;
pub use sources::{
    BlockTextureSource, MAX_TERRAIN_VARIANTS, MaterialTextureSource, normalize_terrain_weights,
};

/// Sidecar wire version; part of the world carrier's prepared identity.
pub const MATERIAL_KEYS_SCHEMA: u32 = 10;
/// Largest sidecar file the runtime reads.
pub const MAX_MATERIAL_KEYS_BYTES: u64 = 16 * 1024 * 1024;

/// Material ids by the terrain texture key they were compiled from.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MaterialKeys {
    keys: BTreeMap<Box<str>, Box<[u32]>>,
    /// Every vanilla terrain key's variant-zero image path.
    aliases: BTreeMap<Box<str>, Box<str>>,
    fixed_tints: BTreeMap<Box<str>, [u8; 3]>,
    terrain: BTreeMap<Box<str>, serde_json::Value>,
    material_sources: BTreeMap<u32, MaterialTextureSource>,
    /// Pixel width and height of each source image named by `material_sources`.
    raster_sizes: BTreeMap<Box<str>, [u32; 2]>,
    block_sources: Vec<BlockTextureSource>,
    flipbooks: Box<[FlipbookSource]>,
    flipbook_selectors: BTreeMap<u32, (u32, u32)>,
    flipbook_destinations: BTreeMap<Box<str>, Box<[u32]>>,
}

impl MaterialKeys {
    /// Groups `(material id, texture key)` pairs by key, ids sorted and unique.
    pub fn from_entries<K: AsRef<str>>(entries: impl IntoIterator<Item = (u32, K)>) -> Self {
        let mut grouped = BTreeMap::<Box<str>, Vec<u32>>::new();
        for (material, key) in entries {
            grouped
                .entry(key.as_ref().into())
                .or_default()
                .push(material);
        }
        let keys = grouped
            .into_iter()
            .map(|(key, mut ids)| {
                ids.sort_unstable();
                ids.dedup();
                (key, ids.into_boxed_slice())
            })
            .collect();
        Self {
            keys,
            aliases: BTreeMap::new(),
            fixed_tints: BTreeMap::new(),
            terrain: BTreeMap::new(),
            material_sources: BTreeMap::new(),
            raster_sizes: BTreeMap::new(),
            block_sources: Vec::new(),
            flipbooks: Box::default(),
            flipbook_selectors: BTreeMap::new(),
            flipbook_destinations: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_aliases<K: AsRef<str>, P: AsRef<str>>(
        mut self,
        aliases: impl IntoIterator<Item = (K, P)>,
    ) -> Self {
        self.aliases = aliases
            .into_iter()
            .map(|(key, path)| (key.as_ref().into(), path.as_ref().into()))
            .collect();
        for key in self.aliases.keys() {
            self.flipbook_destinations
                .entry(key.clone())
                .or_insert_with(|| vec![1].into());
        }
        self
    }

    #[must_use]
    pub fn with_fixed_tints<K: AsRef<str>>(
        mut self,
        tints: impl IntoIterator<Item = (K, [u8; 3])>,
    ) -> Self {
        self.fixed_tints = tints
            .into_iter()
            .map(|(key, tint)| (key.as_ref().into(), tint))
            .collect();
        self
    }

    /// Literal atlas multipliers already baked into the base material textures.
    pub fn fixed_tints(&self) -> impl Iterator<Item = (&str, [u8; 3])> {
        self.fixed_tints
            .iter()
            .map(|(key, tint)| (key.as_ref(), *tint))
    }

    /// Retains the base pack's animation descriptions beneath optional pack layers.
    #[must_use]
    pub fn with_flipbooks(mut self, flipbooks: &[FlipbookSource]) -> Self {
        self.flipbooks = flipbooks.into();
        self
    }

    /// Records each material's atlas index and UV variant for animation replacement.
    #[must_use]
    pub fn with_flipbook_selectors(
        mut self,
        selectors: impl IntoIterator<Item = (u32, (u32, u32))>,
    ) -> Self {
        self.flipbook_selectors = selectors.into_iter().collect();
        self
    }

    /// Base animation descriptions with their original image routes and timelines.
    pub fn flipbooks(&self) -> &[FlipbookSource] {
        &self.flipbooks
    }

    /// Atlas destination for a material; ordinary unindexed tiles use `(0, 0)`.
    pub fn flipbook_selector(&self, material: u32) -> (u32, u32) {
        self.flipbook_selectors
            .get(&material)
            .copied()
            .unwrap_or_default()
    }

    /// Recorded destinations, used when copying carrier materials.
    pub fn flipbook_selectors(&self) -> impl Iterator<Item = (u32, (u32, u32))> + '_ {
        self.flipbook_selectors
            .iter()
            .map(|(&material, &selector)| (material, selector))
    }

    /// Records the number of UV variants at each atlas index, including unused destinations.
    #[must_use]
    pub fn with_flipbook_destinations<K: AsRef<str>, C: AsRef<[u32]>>(
        mut self,
        destinations: impl IntoIterator<Item = (K, C)>,
    ) -> Self {
        self.flipbook_destinations = destinations
            .into_iter()
            .map(|(key, counts)| (key.as_ref().into(), counts.as_ref().into()))
            .collect();
        self
    }

    /// Atlas index and UV-variant bounds derived from the complete base terrain catalog.
    pub fn flipbook_destinations(&self) -> impl Iterator<Item = (&str, &[u32])> {
        self.flipbook_destinations
            .iter()
            .map(|(key, counts)| (key.as_ref(), counts.as_ref()))
    }

    /// Material ids compiled from `key`; empty when the key is unknown.
    #[must_use]
    pub fn materials(&self, key: &str) -> &[u32] {
        self.keys.get(key).map_or(&[], |ids| ids)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.keys.keys().map(AsRef::as_ref)
    }

    /// `(terrain key, image path)` for every vanilla catalog entry.
    pub fn aliases(&self) -> impl Iterator<Item = (&str, &str)> {
        self.aliases
            .iter()
            .map(|(key, path)| (key.as_ref(), path.as_ref()))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

#[cfg(test)]
mod tests;
