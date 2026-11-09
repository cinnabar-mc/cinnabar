//! Piston arm motion clocked by the client simulation.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use super::ChunkStore;
use crate::{
    BlockEntityKey, BlockEntityNbt, Chunk, ChunkKey, CollisionRevisionError, PistonMotion,
};

#[cfg(test)]
mod tests;

impl ChunkStore {
    /// The last client simulation tick reported through [`Self::set_simulation_tick`].
    #[must_use]
    pub const fn simulation_tick(&self) -> u64 {
        self.simulation_tick
    }

    /// Records that the client has simulated `tick`. Piston arm actors that arrive later
    /// continue from the progress their arm reached by then.
    ///
    /// Every loaded column whose arm progress differs between the previous tick and `tick`
    /// takes a new collision revision. A clock that restarts lower re-anchors every arm at
    /// the progress it had reached.
    pub fn set_simulation_tick(&mut self, tick: u64) -> Result<(), CollisionRevisionError> {
        let previous = self.simulation_tick;
        if tick < previous {
            return self.rebase_pistons(previous, tick);
        }
        let mut changed = BTreeSet::new();
        let chunks = &self.chunks;
        self.moving_pistons.retain(|key| {
            let Some(motion) = piston_motion(chunks, *key) else {
                return false;
            };
            if motion.step_at(previous).progress != motion.step_at(tick).progress {
                changed.insert(key.chunk());
            }
            motion.settled_after() > tick
        });
        self.simulation_tick = tick;
        self.revise_columns(changed)
    }

    /// The piston arm actors stored at positions from `min` to `max` inclusive.
    pub fn piston_actors_in(
        &self,
        dimension: i32,
        min: [i32; 3],
        max: [i32; 3],
    ) -> impl Iterator<Item = (BlockEntityKey, PistonMotion)> + '_ {
        let columns = (min[0] >> 4..=max[0] >> 4)
            .flat_map(move |x| (min[2] >> 4..=max[2] >> 4).map(move |z| (x, z)));
        columns
            .filter_map(move |(x, z)| self.chunks.get(&ChunkKey::new(dimension, x, z)))
            .flat_map(move |chunk| {
                (min[0]..=max[0]).flat_map(move |x| {
                    chunk.block_entities.range(
                        BlockEntityKey::new(dimension, x, min[1], i32::MIN)
                            ..=BlockEntityKey::new(dimension, x, max[1], i32::MAX),
                    )
                })
            })
            .filter(move |(key, _)| (min[2]..=max[2]).contains(&key.z))
            .filter_map(|(&key, nbt)| Some((key, *nbt.piston_motion()?)))
    }

    /// Anchors the piston arm actors of a replacement for `chunk` at the simulation tick.
    pub(super) fn report_pistons(
        &mut self,
        chunk: ChunkKey,
        entities: &mut BTreeMap<BlockEntityKey, Arc<BlockEntityNbt>>,
    ) {
        let moving = report(self.chunks.get(&chunk), self.simulation_tick, entities);
        self.moving_pistons.extend(moving);
    }

    /// Re-anchors every stored arm at `tick` with the progress it had reached by `from`.
    fn rebase_pistons(&mut self, from: u64, tick: u64) -> Result<(), CollisionRevisionError> {
        let columns: Vec<ChunkKey> = self
            .chunks
            .iter()
            .filter(|(_, chunk)| {
                chunk
                    .block_entities
                    .values()
                    .any(|nbt| nbt.piston_motion().is_some())
            })
            .map(|(&key, _)| key)
            .collect();
        for &column in &columns {
            let chunk = self
                .chunks
                .edit(&mut self.content_revision)
                .get_mut(&column)
                .expect("listed columns are stored");
            for (&key, nbt) in Arc::make_mut(&mut chunk.block_entities) {
                if let Some(motion) = nbt.piston_motion() {
                    let rebased = motion.rebased(from, tick);
                    Arc::make_mut(nbt).set_piston_motion(rebased);
                    self.moving_pistons.insert(key);
                }
            }
        }
        self.simulation_tick = tick;
        self.revise_columns(columns)
    }

    /// Gives each loaded column a new collision revision, allocating every one first.
    fn revise_columns(
        &mut self,
        columns: impl IntoIterator<Item = ChunkKey>,
    ) -> Result<(), CollisionRevisionError> {
        let revisions = columns
            .into_iter()
            .filter(|column| self.collision_revisions.contains_key(column))
            .map(|column| Ok((column, self.collision_revision_allocator.allocate()?)))
            .collect::<Result<Vec<_>, CollisionRevisionError>>()?;
        for (column, revision) in revisions {
            self.apply_reserved_revision(column, Some(revision));
        }
        Ok(())
    }
}

/// Anchors each piston arm actor in `entities` at `tick`, continuing the motion of the actor
/// it replaces in `previous`, and returns their keys. Motions that do not change keep their
/// shared snapshot.
pub(super) fn report(
    previous: Option<&Chunk>,
    tick: u64,
    entities: &mut BTreeMap<BlockEntityKey, Arc<BlockEntityNbt>>,
) -> Vec<BlockEntityKey> {
    let mut reported = Vec::new();
    for (&key, nbt) in entities.iter_mut() {
        let Some(&current) = nbt.piston_motion() else {
            continue;
        };
        let replaced = previous
            .and_then(|chunk| chunk.block_entities.get(&key))
            .and_then(|previous| previous.piston_motion());
        if replaced != Some(&current) {
            Arc::make_mut(nbt).report_piston(replaced, tick);
        }
        reported.push(key);
    }
    reported
}

/// The stored motion of the arm actor at `key`.
fn piston_motion(
    chunks: &hashbrown::HashMap<ChunkKey, Chunk>,
    key: BlockEntityKey,
) -> Option<PistonMotion> {
    chunks
        .get(&key.chunk())?
        .block_entities
        .get(&key)?
        .piston_motion()
        .copied()
}
