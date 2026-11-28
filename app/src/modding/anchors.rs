//! Current actor authorization and root poses copied from the actor renderer's publication.
use super::{ModRuntime, gameplay::GameplayContext};
use bevy::prelude::*;
use client_extensions::modding::multi::ModHosts;
use mod_render::retained::EntityKey;
use render_scene::mod_scene::retained::{DrawingFrame, RootPose};
use std::collections::BinaryHeap;

/// Supplies bounded nearby candidates; confirmed removal is distinct from unloaded terrain.
pub(super) fn reconcile(hosts: &mut ModHosts, context: &GameplayContext<'_>) {
    if !(0..hosts.host_count())
        .any(|index| hosts.host(index).is_active() && hosts.host(index).grants().entity_observe)
    {
        return;
    }
    let authority = context
        .world
        .as_ref()
        .and_then(|world| world.stream.as_ref())
        .map(|stream| stream.authority());
    let Some(authority) = authority else { return };
    let dimension = authority.current_dimension();
    let session = authority.actor_session_id();
    let candidates = context
        .view
        .as_ref()
        .map(|view| candidates(authority, view.feet_translation().to_array()))
        .unwrap_or_default();
    for index in 0..hosts.host_count() {
        let host = hosts.host_mut(index);
        let removed: Vec<_> = host
            .drawing_entity_keys()
            .filter(|key| {
                key.session != session
                    || key.dimension != dimension
                    || authority
                        .actor(key.runtime_id)
                        .is_none_or(|actor| actor.render_spawn_revision.get() != key.spawn_revision)
            })
            .collect();
        let candidates = if host.grants().entity_observe {
            candidates.clone()
        } else {
            Vec::new()
        };
        if let Err(error) = host.update_drawing_entities(candidates, &removed) {
            warn!("Drawing entity admission failed: {}", error.detail);
        }
    }
}

/// Selects the same bounded nearest actor references for every drawing context.
pub(crate) fn candidates(
    authority: &client_world::WorldAuthority,
    origin: [f32; 3],
) -> Vec<EntityKey> {
    let mut nearest = BinaryHeap::new();
    for actor in authority.remote_actors() {
        let distance = actor
            .position
            .iter()
            .zip(origin)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>();
        if !distance.is_finite()
            || distance > mod_api::MAX_DRAW_ENTITY_RANGE_BLOCKS.powi(2)
            || actor.render_spawn_revision.get() == 0
        {
            continue;
        }
        nearest.push((
            distance.to_bits(),
            actor.runtime_id,
            actor.render_spawn_revision.get(),
        ));
        if nearest.len() > mod_api::MAX_DRAW_ENTITIES {
            nearest.pop();
        }
    }
    nearest
        .into_sorted_vec()
        .into_iter()
        .map(|(_, runtime_id, spawn_revision)| EntityKey {
            session: authority.actor_session_id(),
            dimension: authority.current_dimension(),
            runtime_id,
            spawn_revision,
        })
        .collect()
}

/// Publishes only exact, still-live body roots after the actor renderer's same-frame commit.
pub(super) fn publish(
    runtime: Option<ResMut<ModRuntime>>,
    packet_drawing: Option<ResMut<super::packet_render::Drawing>>,
    context: GameplayContext,
    mut scene: ResMut<render_scene::ModRenderScene>,
    actors: Option<Res<render_scene::ActorRenderFrame>>,
    cameras: Query<&Transform, With<client_presentation::camera::FlyCamera>>,
) {
    let mut packet_drawing = packet_drawing;
    if let (Some(drawing), Some(actors), Ok(camera)) = (
        packet_drawing.as_deref_mut(),
        actors.as_deref(),
        cameras.single(),
    ) {
        drawing.follow_anchors(
            actors,
            &GlobalTransform::from(*camera),
            |session, dimension, runtime_id| {
                let authority = context.world.as_ref()?.stream.as_ref()?.authority();
                if authority.actor_session_id() != session
                    || authority.current_dimension() != dimension
                {
                    return None;
                }
                authority.actor(runtime_id)?.interpolated_position(
                    context
                        .actor_partial
                        .as_deref()
                        .map_or(0., |partial| partial.0),
                )
            },
        );
    }
    let Some(mut runtime) = runtime else {
        if let Some(packet_drawing) = packet_drawing.as_deref()
            && super::packet_render::publish_alone(scene.bypass_change_detection(), packet_drawing)
        {
            scene.set_changed();
        }
        return;
    };
    let changed = super::drawing_policy::bind(&mut runtime.hosts, &context);
    if changed {
        scene.clear();
        runtime.render_sources.clear();
    }
    reconcile(&mut runtime.hosts, &context);
    super::render::publish(Some(scene), &mut runtime, packet_drawing.as_deref());
}

/// Rebuilds availability once from every current contributor, even without personal publishers.
pub(crate) fn publish_frame(
    scene: Res<render_scene::ModRenderScene>,
    world: Res<crate::runtime::world::ClientWorld>,
    actors: Option<Res<render_scene::ActorRenderFrame>>,
    mut published: ResMut<DrawingFrame>,
    cameras: Query<&Transform, With<client_presentation::camera::FlyCamera>>,
    mut next: Local<DrawingFrame>,
) {
    rebuild_drawing_frame(&scene, &world, actors.as_deref(), &mut next, &cameras);
    // The frame is extracted whenever it counts as changed, so it is replaced only when this
    // frame's availability differs; the previous frame becomes the next frame's scratch.
    if *published != *next {
        std::mem::swap(&mut *published, &mut *next);
    }
}

/// Rebuilds this frame's drawing availability into `drawing`, reusing its allocations.
fn rebuild_drawing_frame(
    scene: &render_scene::ModRenderScene,
    world: &crate::runtime::world::ClientWorld,
    actors: Option<&render_scene::ActorRenderFrame>,
    drawing: &mut DrawingFrame,
    cameras: &Query<&Transform, With<client_presentation::camera::FlyCamera>>,
) {
    let authority = world
        .stream
        .as_ref()
        .filter(|_| !world.dimension_transfer.active())
        .map(|stream| stream.authority());
    super::drawing_policy::rebuild_frame(&scene.retained, authority, drawing);
    let Some(authority) = authority else { return };
    for resources in &scene.retained {
        if resources.connection != drawing.connection || resources.dimension != drawing.dimension {
            continue;
        }
        let Some(actors) = actors else {
            continue;
        };
        let server = scene.is_server_contributor(resources);
        for (handle, key) in &resources.entities {
            let Some(actor) = authority.actor(key.runtime_id).filter(|actor| {
                actor.render_spawn_revision.get() == key.spawn_revision
                    && key.session == authority.actor_session_id()
                    && key.dimension == authority.current_dimension()
            }) else {
                continue;
            };
            if let Some(root) = RootPose::from_actor_frame(actors, *key) {
                if !server {
                    let Ok(camera) = cameras.single() else {
                        continue;
                    };
                    let chunk = world::ChunkKey::new(
                        key.dimension,
                        (actor.position[0].floor() as i32).div_euclid(world::SUB_CHUNK_SIDE as i32),
                        (actor.position[2].floor() as i32).div_euclid(world::SUB_CHUNK_SIDE as i32),
                    );
                    let position = Vec3::new(
                        root.world_from_actor[0][3],
                        root.world_from_actor[1][3],
                        root.world_from_actor[2][3],
                    );
                    if !authority.terrain().is_chunk_loaded(chunk)
                        || position.distance_squared(camera.translation)
                            > mod_api::MAX_DRAW_ENTITY_RANGE_BLOCKS.powi(2)
                    {
                        continue;
                    }
                }
                drawing.entity_poses.insert(*handle, root);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frame is extracted whenever it counts as changed, so an unchanged rebuild must not
    /// mark it, while a stale published frame is still replaced.
    #[test]
    fn unchanged_availability_leaves_the_drawing_frame_unchanged() {
        #[derive(Resource, Default)]
        struct Seen(Vec<bool>);
        let mut app = App::new();
        app.init_resource::<render_scene::ModRenderScene>()
            .init_resource::<crate::runtime::world::ClientWorld>()
            .init_resource::<DrawingFrame>()
            .init_resource::<Seen>()
            .add_systems(
                Update,
                (
                    publish_frame,
                    |frame: Res<DrawingFrame>, mut seen: ResMut<Seen>| {
                        seen.0.push(frame.is_changed());
                    },
                )
                    .chain(),
            );
        for _ in 0..3 {
            app.update();
        }
        app.world_mut()
            .resource_mut::<DrawingFrame>()
            .loaded_blocks
            .insert([0; 3]);
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<Seen>().0,
            [true, false, false, true, false]
        );
        assert!(
            app.world()
                .resource::<DrawingFrame>()
                .loaded_blocks
                .is_empty()
        );
    }
}
