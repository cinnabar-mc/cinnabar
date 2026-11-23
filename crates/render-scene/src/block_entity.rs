//! Bevy resources for CPU-built block geometry and static item placements.
use bevy::{
    prelude::{Deref, DerefMut, Resource},
    render::extract_resource::ExtractResource,
};

/// Mutable block-model cache owned by the main world.
#[derive(Debug, Default, Resource, Deref, DerefMut)]
pub struct BlockEntitySceneResource(pub render_model::BlockEntityScene);

/// Immutable block vertices extracted for the GPU pass.
#[derive(Clone, Debug, Default, Resource, ExtractResource, Deref, DerefMut)]
#[extract_app(bevy::render::RenderApp)]
pub struct BlockEntityFrameResource(pub render_model::BlockEntityFrame);

impl BlockEntityFrameResource {
    /// Publishes `next` unless it repeats the published geometry without portals, the only
    /// layer that reads the frame clock, so extraction skips identical frames.
    pub fn publish(
        frame: &mut impl std::ops::DerefMut<Target = Self>,
        next: &render_model::BlockEntityFrame,
    ) {
        if next.revision != frame.revision || !next.portal.is_empty() {
            frame.0 = next.clone();
        }
    }
}

/// Selection geometry extracted for the overlay pass.
#[derive(Clone, Debug, Default, Resource, ExtractResource, Deref, DerefMut)]
#[extract_app(bevy::render::RenderApp)]
pub struct BlockSelectionFrameResource(pub render_model::BlockSelectionFrame);

/// Static dropped-item placements published with block-entity models.
#[derive(Clone, Debug, Default, PartialEq, Resource, Deref, DerefMut)]
pub struct StaticItemPlacementsResource(pub render_model::StaticItemPlacements);

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        ecs::change_detection::DetectChanges,
        prelude::{ResMut, World},
    };

    /// Publishes `next` through a fresh change tick and reports whether the resource changed.
    fn publish(world: &mut World, next: &render_model::BlockEntityFrame) -> bool {
        world.clear_trackers();
        world
            .run_system_cached_with(
                |next: bevy::prelude::In<render_model::BlockEntityFrame>,
                 mut frame: ResMut<BlockEntityFrameResource>| {
                    BlockEntityFrameResource::publish(&mut frame, &next);
                },
                next.clone(),
            )
            .unwrap();
        world
            .resource_ref::<BlockEntityFrameResource>()
            .is_changed()
    }

    #[test]
    fn only_new_geometry_or_portal_clocks_republish_the_frame() {
        let mut world = World::new();
        world.init_resource::<BlockEntityFrameResource>();
        let mut next = render_model::BlockEntityFrame {
            revision: 1,
            ..Default::default()
        };
        assert!(publish(&mut world, &next), "new geometry");
        next.portal_time_seconds = 2.0;
        assert!(!publish(&mut world, &next), "a clock without portals");
        assert_eq!(
            world
                .resource::<BlockEntityFrameResource>()
                .portal_time_seconds,
            0.0
        );
        next.portal = vec![Default::default()].into();
        assert!(publish(&mut world, &next), "a portal reads the clock");
        next.revision = 2;
        next.portal = Vec::new().into();
        assert!(publish(&mut world, &next), "new geometry");
    }
}
