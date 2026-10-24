//! Sidecar serialization and validation.

use super::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    schema: u32,
    materials: u32,
    keys: BTreeMap<String, Vec<u32>>,
    aliases: BTreeMap<String, String>,
    fixed_tints: BTreeMap<String, [u8; 3]>,
    terrain: BTreeMap<Box<str>, serde_json::Value>,
    material_sources: BTreeMap<u32, MaterialTextureSource>,
    raster_sizes: BTreeMap<String, [u32; 2]>,
    block_sources: Vec<BlockTextureSource>,
    flipbooks: Vec<FlipbookSource>,
    flipbook_selectors: BTreeMap<u32, (u32, u32)>,
    flipbook_destinations: BTreeMap<String, Vec<u32>>,
}

impl MaterialKeys {
    /// Serializes the sidecar for a carrier with `material_count` materials.
    #[must_use]
    pub fn to_json(&self, material_count: u32) -> Vec<u8> {
        let wire = Wire {
            schema: MATERIAL_KEYS_SCHEMA,
            materials: material_count,
            keys: self
                .keys
                .iter()
                .map(|(key, ids)| (key.to_string(), ids.to_vec()))
                .collect(),
            aliases: self
                .aliases
                .iter()
                .map(|(key, path)| (key.to_string(), path.to_string()))
                .collect(),
            terrain: self.terrain.clone(),
            material_sources: self.material_sources.clone(),
            raster_sizes: self
                .raster_sizes
                .iter()
                .map(|(raster, size)| (raster.to_string(), *size))
                .collect(),
            block_sources: self.block_sources.clone(),
            fixed_tints: self
                .fixed_tints
                .iter()
                .map(|(key, tint)| (key.to_string(), *tint))
                .collect(),
            flipbooks: self.flipbooks.to_vec(),
            flipbook_selectors: self.flipbook_selectors.clone(),
            flipbook_destinations: self
                .flipbook_destinations
                .iter()
                .map(|(key, counts)| (key.to_string(), counts.to_vec()))
                .collect(),
        };
        serde_json::to_vec(&wire).expect("material key sidecar serializes")
    }

    /// Parses a sidecar; `None` unless it matches a carrier of `material_count`
    /// materials exactly.
    #[must_use]
    pub fn from_json(bytes: &[u8], material_count: usize) -> Option<Self> {
        let wire: Wire = serde_json::from_slice(bytes).ok()?;
        if wire.schema != MATERIAL_KEYS_SCHEMA || wire.materials as usize != material_count {
            return None;
        }
        if wire.flipbooks.iter().any(|flipbook| !flipbook.is_valid())
            || wire
                .flipbook_selectors
                .keys()
                .any(|&id| id as usize >= material_count)
            || wire
                .flipbook_destinations
                .values()
                .any(|counts| counts.is_empty() || counts.contains(&0))
            || wire
                .material_sources
                .keys()
                .any(|&id| id as usize >= material_count)
            || wire.block_sources.iter().any(|source| {
                let mut faces = source.source_faces;
                faces.sort_unstable();
                source
                    .source_faces
                    .iter()
                    .any(|&face| face >= crate::BlockFace::ALL.len() as u8)
                    || (source.cube_binding_eligible
                        && faces != crate::BlockFace::ALL.map(|face| face as u8))
            })
        {
            return None;
        }
        let mut keys = BTreeMap::new();
        for (key, ids) in wire.keys {
            if ids.iter().any(|&id| id as usize >= material_count) {
                return None;
            }
            keys.insert(key.into_boxed_str(), ids.into_boxed_slice());
        }
        let aliases = wire
            .aliases
            .into_iter()
            .map(|(key, path)| (key.into_boxed_str(), path.into_boxed_str()))
            .collect();
        let fixed_tints = wire
            .fixed_tints
            .into_iter()
            .map(|(key, tint)| (key.into_boxed_str(), tint))
            .collect();
        Some(
            Self {
                keys,
                aliases,
                fixed_tints,
                flipbooks: wire.flipbooks.into_boxed_slice(),
                flipbook_selectors: wire.flipbook_selectors,
                flipbook_destinations: wire
                    .flipbook_destinations
                    .into_iter()
                    .map(|(key, counts)| (key.into_boxed_str(), counts.into_boxed_slice()))
                    .collect(),
                ..Self::default()
            }
            .with_sources(wire.terrain, wire.material_sources, wire.block_sources)
            .with_raster_sizes(
                wire.raster_sizes
                    .into_iter()
                    .map(|(raster, size)| (raster.into_boxed_str(), size)),
            ),
        )
    }
}
