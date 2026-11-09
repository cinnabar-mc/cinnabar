//! Piston arms that push movers.
//!
//! Each tick that a native client piston actor's progress changes (`PistonBlockActor::tick`),
//! `_moveCollidedEntities` pushes the actors inside the one-block cube at the arm's new
//! progress: the piston's position moved `progress` blocks along its facing.

use std::collections::BTreeSet;

use assets::block_shapes::PISTON_DIRECTIONS;
use world::ChunkKey;

use super::{Aabb, CollisionQuery, PaletteWorld, Vec3, WorldQueryError, block_floor};

/// A piston arm that moved on the simulated tick and pushes what its cube overlaps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PistonPush {
    /// The block the arm's head sweeps at its new progress.
    pub cube: Aabb,
    /// The piston's facing, along which it pushes even while retracting.
    pub direction: [i32; 3],
}

impl PistonPush {
    /// The move that takes `mover` clear of the cube, as native
    /// `_moveCollidedEntitiesHelper` computes it in `f32`: the first nonzero facing axis
    /// carries the mover's trailing face to the cube's leading face. `None` when the mover
    /// does not overlap the cube.
    #[must_use]
    pub fn delta(&self, mover: Aabb) -> Option<Vec3> {
        if !self.cube.intersects(mover) {
            return None;
        }
        let axis = (0..3).find(|&axis| self.direction[axis] != 0)?;
        let distance = if self.direction[axis] > 0 {
            self.cube.max[axis] as f32 - mover.min[axis] as f32
        } else {
            self.cube.min[axis] as f32 - mover.max[axis] as f32
        };
        let mut delta = Vec3::ZERO;
        delta[axis] = f64::from(distance);
        Some(delta)
    }
}

impl PaletteWorld<'_> {
    /// The arms whose progress changes on `tick`'s update and whose cube at the new progress
    /// can overlap `query`, in store order. Arms in unloaded columns, or whose piston block
    /// cannot be read, push nothing.
    pub(super) fn moved_piston_arms(
        &self,
        query: Aabb,
        tick: u64,
    ) -> Result<CollisionQuery<Vec<PistonPush>>, WorldQueryError> {
        super::validate_collision_query(query)?;
        // A cube lies at most one block from its piston along each axis.
        let min = block_floor(query.min)?.map(|value| value.saturating_sub(1));
        let max = block_floor(query.max)?.map(|value| value.saturating_add(1));
        let mut pushes = Vec::new();
        let mut columns = BTreeSet::new();
        for (key, motion) in self.store.piston_actors_in(self.dimension, min, max) {
            let column = ChunkKey::new(self.dimension, key.x >> 4, key.z >> 4);
            if self.store.collision_revision(column).is_none() {
                continue;
            }
            columns.insert(column);
            let step = motion.step_at(tick);
            if !step.moved() {
                continue;
            }
            let position = [key.x, key.y, key.z];
            let Some(facing) = self
                .primary_runtime_id(position)
                .ok()
                .and_then(|id| self.registry.physics(id))
                .and_then(|physics| physics.actor_facing)
            else {
                continue;
            };
            let direction = PISTON_DIRECTIONS[usize::from(facing)];
            let min = std::array::from_fn::<_, 3, _>(|axis| {
                step.progress * direction[axis] as f32 + position[axis] as f32
            });
            pushes.push(PistonPush {
                cube: Aabb::new(
                    Vec3::new(min[0].into(), min[1].into(), min[2].into()),
                    Vec3::new(
                        (min[0] + 1.0).into(),
                        (min[1] + 1.0).into(),
                        (min[2] + 1.0).into(),
                    ),
                ),
                direction,
            });
        }
        Ok(CollisionQuery {
            value: pushes,
            identity: self.identity_for_chunks(columns)?,
        })
    }
}

#[cfg(test)]
mod tests;
