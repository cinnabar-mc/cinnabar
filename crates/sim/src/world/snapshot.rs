use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use super::*;

/// An immutable collision world retained for one prediction frame.
///
/// The retained terrain is shared between clones; each clone keeps its own simulation tick,
/// so replay can show piston arms at the progress of the tick it replays.
#[derive(Debug)]
pub struct CollisionSnapshot {
    data: Arc<SnapshotData>,
    tick: AtomicU64,
}

#[derive(Debug)]
struct SnapshotData {
    store: Arc<ChunkStore>,
    registry: CollisionRegistry,
    dimension: i32,
    unloaded_collision_barriers: bool,
    actor_colliders: Option<Arc<[Aabb]>>,
}

impl Clone for CollisionSnapshot {
    fn clone(&self) -> Self {
        Self {
            data: Arc::clone(&self.data),
            tick: AtomicU64::new(self.tick.load(Ordering::Relaxed)),
        }
    }
}

impl PartialEq for CollisionSnapshot {
    /// Snapshots are equal when they retain the same captured world.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.data, &other.data)
    }
}

impl CollisionSnapshot {
    /// Keeps the original palette pages alive after updates or chunk unloads.
    /// Retains the world's simulation tick as well.
    pub(super) fn capture(world: &PaletteWorld<'_>) -> Self {
        Self {
            data: Arc::new(SnapshotData {
                store: world.store.collision_snapshot(),
                registry: world.registry.clone(),
                dimension: world.dimension,
                unloaded_collision_barriers: world.unloaded_collision_barriers,
                actor_colliders: world.actor_colliders.clone(),
            }),
            tick: AtomicU64::new(world.tick()),
        }
    }

    /// Borrows the ordinary palette adapter so replay uses the live query implementation.
    fn world(&self) -> PaletteWorld<'_> {
        let data = &self.data;
        let mut world = PaletteWorld::new(
            &data.store,
            &data.registry,
            data.dimension,
            Default::default(),
        );
        world.unloaded_collision_barriers = data.unloaded_collision_barriers;
        world.actor_colliders = data.actor_colliders.clone();
        world.begin_tick(self.tick.load(Ordering::Relaxed));
        world
    }
}

impl CollisionWorld for CollisionSnapshot {
    /// Checks the origin against the terrain retained by the prediction frame.
    fn ensure_movement_origin(&self, feet: Vec3) -> Result<(), WorldQueryError> {
        self.world().ensure_movement_origin(feet)
    }

    fn registry_identity(&self) -> CollisionRegistryIdentity {
        self.data.registry.identity()
    }

    fn begin_tick(&self, tick: u64) {
        self.tick.store(tick, Ordering::Relaxed);
    }

    fn piston_pushes(
        &self,
        query: Aabb,
        tick: u64,
    ) -> Result<Option<CollisionQuery<Vec<PistonPush>>>, WorldQueryError> {
        self.world().piston_pushes(query, tick)
    }

    fn liquid_current(
        &self,
        previous_pose: Aabb,
    ) -> Result<Option<CollisionQuery<Vec3>>, WorldQueryError> {
        self.world().liquid_current(previous_pose)
    }

    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        self.world().collision_boxes(query)
    }

    fn collision_boxes_with_provenance(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<Vec<ProvenancedCollider>>, WorldQueryError> {
        self.world().collision_boxes_with_provenance(query)
    }

    fn collision_boxes_for_mover(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<MoverColliders>, WorldQueryError> {
        self.world().collision_boxes_for_mover(query)
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        self.world().block_physics(block)
    }

    /// Uses the retained palette pages without constructing material-sample identities.
    fn visit_block_materials(
        &self,
        block: [i32; 3],
        visitor: &mut dyn FnMut(BlockPhysicsFacts),
    ) -> Result<(), WorldQueryError> {
        self.world().visit_block_materials(block, visitor)
    }

    fn primary_is_air(
        &self,
        block: [i32; 3],
    ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
        self.world().primary_is_air(block)
    }

    fn snapshot(&self) -> Option<CollisionSnapshot> {
        Some(self.clone())
    }
}
