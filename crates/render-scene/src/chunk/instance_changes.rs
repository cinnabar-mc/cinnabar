//! Entities whose [`ChunkRenderInstance`] was inserted or replaced since its consumer last ran.
use bevy::ecs::{lifecycle::HookContext, world::DeferredWorld};
use bevy::prelude::{Entity, Resource};
use std::sync::Mutex;

#[cfg(doc)]
use super::ChunkRenderInstance;

/// Records each entity that receives a [`ChunkRenderInstance`], in the world holding this
/// resource.
///
/// The main world's extraction and the render world's GPU preparation each take the marks
/// instead of scanning every instance for change ticks. Insertions mark through the
/// component's insert hook. Code that mutates an instance in place must call
/// [`ChunkInstanceChanges::mark`]; debug builds compare the marks with change detection.
#[derive(Resource, Default, Debug)]
pub struct ChunkInstanceChanges {
    marked: Mutex<Vec<Entity>>,
}

impl ChunkInstanceChanges {
    /// Records an in-place change to `entity`'s instance.
    pub fn mark(&self, entity: Entity) {
        self.marked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(entity);
    }

    /// Moves every mark since the previous take into `out`, sorted and without duplicates.
    ///
    /// `out` is cleared first and its allocation is reused for later marks.
    pub fn take_into(&self, out: &mut Vec<Entity>) {
        out.clear();
        std::mem::swap(
            &mut *self
                .marked
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            out,
        );
        out.sort_unstable();
        out.dedup();
    }
}

/// Marks an inserted or replaced instance in the world's [`ChunkInstanceChanges`], if any.
pub(super) fn mark_inserted_chunk_instance(world: DeferredWorld, context: HookContext) {
    if let Some(changes) = world.get_resource::<ChunkInstanceChanges>() {
        changes.mark(context.entity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChunkUploadPriority;
    use crate::chunk::*;
    use bevy::prelude::World;

    /// An empty instance; only its insertion matters here.
    fn instance() -> ChunkRenderInstance {
        ChunkRenderInstance {
            key: SubChunkKey::new(0, 0, 0, 0),
            origin: [0; 3],
            generation: 0,
            cube_layout: CubeQuadLayout::default(),
            cube_quads: Arc::from([]),
            cube_lighting: Arc::from([]),
            model_refs: Arc::from([]),
            model_lighting: Arc::from([]),
            model_draw_refs: Arc::from([]),
            transparent_model_draw_refs: Arc::from([]),
            liquid_quads: Arc::from([]),
            liquid_lighting: Arc::from([]),
            has_depth_liquid: false,
            has_transparent_liquid: false,
            depth_liquid_start: None,
            biome: PackedBiomeRecord::fallback(),
            tint_identity: ChunkBiomeTintIdentity::default(),
            priority: ChunkUploadPriority::new(0.0),
            token: None,
            publication_permit: None,
        }
    }

    #[test]
    fn insertions_and_replacements_are_taken_once_in_entity_order() {
        let mut world = World::new();
        world.init_resource::<ChunkInstanceChanges>();
        let first = world.spawn(instance()).id();
        let second = world.spawn(instance()).id();
        world.entity_mut(first).insert(instance());
        let unmarked = world.spawn_empty().id();
        world.resource::<ChunkInstanceChanges>().mark(second);

        let mut taken = vec![unmarked];
        world
            .resource::<ChunkInstanceChanges>()
            .take_into(&mut taken);
        let mut expected = vec![first, second];
        expected.sort_unstable();
        assert_eq!(taken, expected);
        world
            .resource::<ChunkInstanceChanges>()
            .take_into(&mut taken);
        assert!(taken.is_empty());
    }

    #[test]
    fn a_world_without_the_resource_ignores_insertions() {
        let mut world = World::new();
        world.spawn(instance());
        world.init_resource::<ChunkInstanceChanges>();
        let mut taken = Vec::new();
        world
            .resource::<ChunkInstanceChanges>()
            .take_into(&mut taken);
        assert!(taken.is_empty());
    }
}
