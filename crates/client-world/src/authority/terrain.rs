use super::WorldAuthority;
use assets::facts::BlockEntityFacts;
use std::collections::BTreeSet;
use world::*;

impl WorldAuthority {
    /// Clears a decoded all-air slot and returns its changed key.
    pub fn apply_all_air(
        &mut self,
        key: SubChunkKey,
    ) -> Result<Option<SubChunkKey>, CollisionRevisionError> {
        self.terrain.apply_all_air(key)
    }
    /// Applies authoritative air for a slot omitted by request-mode terrain.
    pub fn apply_request_mode_air(
        &mut self,
        key: SubChunkKey,
    ) -> Result<Option<SubChunkKey>, CollisionRevisionError> {
        self.terrain.apply_request_mode_air(key)
    }
    /// Replaces a biome column and returns affected subchunks.
    pub fn commit_biome_column(
        &mut self,
        key: ChunkKey,
        replacement: DecodedBiomeColumn,
    ) -> Vec<SubChunkKey> {
        self.terrain.commit_biome_column(key, replacement)
    }
    /// Applies decoded block-entity data and reports whether it changed.
    pub fn commit_block_entity_update(
        &mut self,
        key: BlockEntityKey,
        nbt: BlockEntityNbt,
    ) -> Result<bool, BlockEntityError> {
        self.terrain.commit_block_entity_update(key, nbt)
    }
    /// Records the client simulation tick that piston arm motion follows.
    pub fn set_simulation_tick(&mut self, tick: u64) -> Result<(), CollisionRevisionError> {
        self.terrain.set_simulation_tick(tick)
    }
    /// Replaces the block entities decoded with a terrain column.
    pub fn commit_chunk_block_entities(
        &mut self,
        key: ChunkKey,
        replacement: DecodedBlockEntities,
    ) -> Result<(), CollisionRevisionError> {
        self.terrain.commit_chunk_block_entities(key, replacement)
    }
    /// Commits one decoded subchunk with its collision revision checks.
    pub fn commit_decoded_sub_chunk(
        &mut self,
        key: SubChunkKey,
        decoded: DecodedSubChunk,
    ) -> Result<Option<SubChunkKey>, DecodeError> {
        self.terrain.commit_decoded_sub_chunk(key, decoded)
    }
    /// Commits a decoded column and returns its changed and dirty subchunks.
    pub fn commit_level_chunk(
        &mut self,
        key: ChunkKey,
        decoded: DecodedLevelChunk,
    ) -> Result<ApplyLevelChunk, DecodeError> {
        self.terrain.commit_level_chunk(key, decoded)
    }
    /// Atomically applies prepared block mutations and returns changed subchunks.
    ///
    /// Like vanilla's chunk block setter, a block whose block-entity kind changes drops the
    /// entity it held, and a block that needs one gets an entity holding only its identity
    /// until the server sends its data. Data that already names the new kind is kept, so an
    /// entity update that arrives before its block still applies.
    pub fn commit_prepared_block_updates(
        &mut self,
        prepared: Vec<PreparedSubChunkMutation>,
    ) -> Result<Vec<SubChunkKey>, MutationError> {
        let swaps = self.block_entity_swaps(&prepared);
        let changed = self.terrain.commit_prepared_block_updates(prepared)?;
        for swap in swaps {
            self.apply_block_entity_swap(swap);
        }
        Ok(changed)
    }

    /// The changed cells of `prepared` whose base-layer block entity kind differs before
    /// and after the batch.
    fn block_entity_swaps(&self, prepared: &[PreparedSubChunkMutation]) -> Vec<BlockEntitySwap> {
        let mut swaps = Vec::new();
        for mutation in prepared.iter().filter(|mutation| mutation.changed()) {
            let previous = self.terrain.sub_chunk(mutation.key());
            for position in mutation.changed_positions() {
                let local = [position.x, position.y, position.z].map(|c| c.rem_euclid(16) as u8);
                let entity_at = |sub_chunk: Option<&SubChunk>| {
                    sub_chunk
                        .and_then(|sub_chunk| sub_chunk.runtime_id(0, local[0], local[1], local[2]))
                        .and_then(|runtime_id| self.block_entity_id(runtime_id))
                };
                let before = entity_at(previous.as_deref());
                let after = entity_at(mutation.replacement());
                if before != after {
                    swaps.push(BlockEntitySwap {
                        key: BlockEntityKey::new(
                            mutation.key().dimension,
                            position.x,
                            position.y,
                            position.z,
                        ),
                        before,
                        after,
                    });
                }
            }
        }
        swaps
    }

    /// The block-entity id a block state of this session's registry carries, if any.
    fn block_entity_id(&self, runtime_id: u32) -> Option<&'static str> {
        BlockEntityFacts::pinned()
            .for_registry(
                runtime_id,
                self.network_id_mode,
                self.runtime_assets.provenance().block_registry_sha256,
            )
            .filter(|backing| !backing.entity.idless)
            .map(|backing| backing.entity.nbt_id.as_str())
    }

    /// Drops the entity of the kind the block lost and creates the kind it gained, keeping
    /// data the server already sent for the gained kind.
    fn apply_block_entity_swap(&mut self, swap: BlockEntitySwap) {
        let stored = self.terrain.block_entity(swap.key);
        let stored_id = stored.as_deref().and_then(BlockEntityNbt::id);
        if swap.after.is_some() && stored_id == swap.after {
            return;
        }
        if swap.before.is_some() && stored_id == swap.before {
            // Revision exhaustion leaves the stale entity for the next column replacement.
            let _ = self.terrain.remove_block_entity(swap.key);
        }
        if let Some(after) = swap.after {
            let position = [swap.key.x, swap.key.y, swap.key.z];
            // A chunk at its entity limit keeps the block edit without the entity.
            let _ = self.terrain.commit_block_entity_update(
                swap.key,
                BlockEntityNbt::client_created(after, position),
            );
        }
    }
    /// Commits a packed subchunk and returns its changed key.
    pub fn commit_sub_chunk(
        &mut self,
        key: SubChunkKey,
        decoded: SubChunk,
    ) -> Result<Option<SubChunkKey>, DecodeError> {
        self.terrain.commit_sub_chunk(key, decoded)
    }
    /// Removes retained columns and returns their keys and owned storage.
    pub fn detach_chunks(&mut self, keys: &BTreeSet<ChunkKey>) -> (Vec<SubChunkKey>, Vec<Chunk>) {
        self.terrain.detach_chunks(keys)
    }
    /// Records that a column is loaded, updating collision revisions.
    pub fn mark_chunk_loaded(&mut self, key: ChunkKey) -> Result<bool, CollisionRevisionError> {
        self.terrain.mark_chunk_loaded(key)
    }
    /// Records that a subchunk is loaded, updating collision revisions.
    pub fn mark_sub_chunk_loaded(
        &mut self,
        key: SubChunkKey,
    ) -> Result<bool, CollisionRevisionError> {
        self.terrain.mark_sub_chunk_loaded(key)
    }
    /// Applies a synchronous block prediction with collision revision checks.
    pub fn update_block(
        &mut self,
        key: SubChunkKey,
        update: BlockUpdate,
        air_runtime_id: u32,
    ) -> Result<Option<SubChunkKey>, MutationError> {
        self.terrain.update_block(key, update, air_runtime_id)
    }
}

/// One cell whose block-entity kind a block mutation changed.
struct BlockEntitySwap {
    key: BlockEntityKey,
    before: Option<&'static str>,
    after: Option<&'static str>,
}
