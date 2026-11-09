//! Swapping a worker-decoded column into the store.

use std::{collections::BTreeSet, sync::Arc};

use super::{ApplyLevelChunk, ChunkStore, DecodedLevelChunk, helpers::*};
use crate::{Chunk, ChunkKey, DecodeError, SubChunkKey};

impl ChunkStore {
    /// Atomically swaps a fully worker-decoded column into the store.
    pub fn commit_level_chunk(
        &mut self,
        key: ChunkKey,
        decoded: DecodedLevelChunk,
    ) -> Result<ApplyLevelChunk, DecodeError> {
        let newly_loaded = !self.loaded_chunks.contains(&key);
        let old = self.chunks.get(&key);
        let DecodedLevelChunk {
            mut sub_chunks,
            mut biomes,
            block_entities,
            block_bytes_consumed,
            bytes_consumed,
            work: _,
        } = decoded;
        for (&y, replacement) in &mut sub_chunks {
            if let Some(previous) =
                old.and_then(|chunk| chunk.sub_chunks.get(&y))
                    .filter(|previous| {
                        Arc::ptr_eq(previous, replacement) || previous.blocks_match(replacement)
                    })
            {
                if !Arc::ptr_eq(previous, replacement) {
                    previous.retain_inline_identity_from(replacement);
                }
                *replacement = Arc::clone(previous);
            }
        }
        if biomes.is_none() {
            biomes = old.and_then(|chunk| chunk.biomes.clone());
        } else if let Some(replacement) = biomes.as_mut() {
            reuse_equal_biome_arcs(replacement, old.and_then(|chunk| chunk.biomes.as_ref()));
        }
        let mut block_entities = match block_entities {
            Some(replacement) => replacement.into_entities(),
            None => old
                .map(|chunk| std::collections::BTreeMap::clone(&chunk.block_entities))
                .unwrap_or_default(),
        };
        let moving = super::pistons::report(old, self.simulation_tick, &mut block_entities);
        if let Some(previous) = old {
            for (&entity_key, replacement) in &mut block_entities {
                if let Some(previous) = previous
                    .block_entities
                    .get(&entity_key)
                    .filter(|previous| previous.as_ref() == replacement.as_ref())
                {
                    *replacement = Arc::clone(previous);
                }
            }
        }

        let ys = old
            .into_iter()
            .flat_map(|chunk| chunk.sub_chunks.keys().copied())
            .chain(sub_chunks.keys().copied())
            .collect::<BTreeSet<_>>();
        let block_changed = ys
            .into_iter()
            .filter(|y| {
                let previous = old.and_then(|chunk| chunk.sub_chunks.get(y));
                let replacement = sub_chunks.get(y);
                match (previous, replacement) {
                    (Some(previous), Some(replacement)) => !Arc::ptr_eq(previous, replacement),
                    (None, None) => false,
                    _ => true,
                }
            })
            .map(|y| SubChunkKey::from_chunk(key, y))
            .collect::<BTreeSet<_>>();
        let biome_changed =
            changed_biome_ys(old.and_then(|chunk| chunk.biomes.as_ref()), biomes.as_ref())
                .into_iter()
                .map(|y| SubChunkKey::from_chunk(key, y))
                .collect::<Vec<_>>();
        let collision_changed = !block_changed.is_empty();
        let revision = if newly_loaded || collision_changed {
            Some(self.collision_revision_allocator.allocate()?)
        } else {
            self.reserve_actor_replacement(
                key,
                None,
                block_entities.iter().map(|(&key, nbt)| (key, nbt.as_ref())),
            )?
        };
        let block_changed = block_changed.into_iter().collect::<Vec<_>>();
        let mut changed = block_changed.clone();
        changed.extend(biome_changed.iter().copied());
        changed.sort_unstable();
        changed.dedup();
        let dirty = expand_mesh_dependents(changed.clone())
            .into_iter()
            .chain(expand_biome_mesh_dependents(biome_changed))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        let column = (!sub_chunks.is_empty() || biomes.is_some() || !block_entities.is_empty())
            .then(|| Chunk {
                sub_chunks: if collision_changed {
                    Arc::new(sub_chunks)
                } else {
                    old.map_or_else(
                        || Arc::new(sub_chunks),
                        |chunk| Arc::clone(&chunk.sub_chunks),
                    )
                },
                biomes,
                block_entity_bytes: block_entities
                    .values()
                    .map(|entity| entity.bytes().len())
                    .sum(),
                block_entities: Arc::new(block_entities),
            });
        let chunks = self.chunks.edit(&mut self.content_revision);
        match column {
            Some(column) => {
                chunks.insert(key, column);
            }
            None => {
                chunks.remove(&key);
            }
        }
        if newly_loaded {
            self.loaded_chunks
                .edit(&mut self.content_revision)
                .insert(key);
            self.authoritative_sub_chunks
                .edit(&mut self.content_revision)
                .remove(&key);
        }
        self.apply_reserved_revision(key, revision);
        self.moving_pistons.extend(moving);
        Ok(ApplyLevelChunk {
            block_changed,
            changed,
            dirty,
            bytes_consumed,
            block_bytes_consumed,
        })
    }
}
