//! Steady actor frames must not allocate per drawn actor.

use std::{sync::Arc, time::Duration};
use web_time::Instant;

use bevy::{
    math::Vec3,
    prelude::{PerspectiveProjection, Projection, Transform, World},
    time::{Real, Time},
};
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldEvent};
use {
    render_model::ActorArtworkPages,
    render_scene::{ActorRenderFrame, ActorRenderScene},
};

use super::actor_rest_presentation::{compiled_fixture, stream};
use {
    crate::runtime::network::prepare_actor_render_frame,
    client_presentation::actor_publication::{
        ActorFramePartialTick, HandRigBuilder, publish_actor_render_frame,
    },
};

/// A world holding every resource the actor publication system reads, with a perspective camera
/// at `eye` looking at `target`.
pub(crate) fn actor_frame_world(
    client_world: crate::runtime::world::ClientWorld,
    scene: ActorRenderScene,
    artwork: ActorArtworkPages,
    hand: HandRigBuilder,
    (eye, target): (Vec3, Vec3),
) -> World {
    let mut world = World::new();
    world.insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
        1,
    )));
    world.insert_resource(client_world);
    world.init_resource::<crate::runtime::shutdown::FatalError>();
    world.insert_resource(Time::<Real>::new(Instant::now()));
    world.insert_resource(scene);
    world.insert_resource(ActorRenderFrame::default());
    world.insert_resource(client_presentation::local_player::LocalAvatarPresentation::default());
    world.insert_resource(
        client_presentation::local_player::LocalAvatarVisibilityCarrier::default(),
    );
    world.insert_resource(client_presentation::camera::CameraSettingsAuthority::default());
    world.insert_resource(client_interaction::movement::LocalPhysicsController::default());
    world.insert_resource(render_scene::ActorRuntimeWitness::default());
    world.insert_resource(render_scene::ActorArtworkResource(artwork));
    world.insert_resource(hand);
    world.insert_resource(render_scene::HandRigScene::default());
    world.insert_resource(client_shell::player_skin::LocalPlayerSkin::generated_default("bench"));
    world.init_resource::<client_presentation::actor_publication::ActorFrameState>();
    world.insert_resource(ActorFramePartialTick::default());
    world.init_resource::<client_presentation::actor_publication::PreparedActorPublication>();
    let camera = Transform::from_translation(eye).looking_at(target, Vec3::Y);
    world.spawn((
        camera,
        Projection::Perspective(PerspectiveProjection {
            fov: 70f32.to_radians(),
            aspect_ratio: 16.0 / 9.0,
            ..Default::default()
        }),
        client_presentation::camera::FlyCamera::default(),
    ));
    world.insert_resource(client_presentation::local_player::LocalViewPose::new(
        eye,
        camera.rotation,
    ));
    world
}

fn spawn(runtime_id: u64, position: [f32; 3]) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id: -(runtime_id as i64),
        runtime_id,
        kind: ActorKind::Entity {
            identifier: "minecraft:example".into(),
        },
        position,
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: Default::default(),
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    }))
}

/// A world with `actors` animated actors standing in view of the camera.
fn steady_actor_world(actors: u64) -> World {
    let (_pack, artwork, entities) =
        compiled_fixture("1.0", 1, assets::ActorPoseMode::CompiledLiteral);
    let mut stream = stream(Arc::clone(&entities));
    for index in 0..actors {
        let position = [
            (index % 8) as f32 * 2.0 - 7.0,
            64.0,
            (index / 8) as f32 * 2.0,
        ];
        stream
            .submit(index + 1, spawn(index + 100, position))
            .unwrap();
    }
    let mut scene =
        ActorRenderScene::with_runtime_entity_assets_and_equipment(&entities, &[]).unwrap();
    scene.configure_artwork(artwork.clone());
    let mut client_world = crate::runtime::world::ClientWorld::new_with_entity_assets(
        Arc::new(assets::RuntimeAssets::diagnostic()),
        Arc::clone(&entities),
    );
    client_world.stream = Some(stream);
    actor_frame_world(
        client_world,
        scene,
        artwork,
        HandRigBuilder::from_runtime_assets(&entities).unwrap(),
        (Vec3::new(0.0, 66.0, -12.0), Vec3::new(0.0, 64.0, 4.0)),
    )
}

/// The first actor's last completed animation tick.
fn rig_tick(world: &World) -> Option<u64> {
    world
        .resource::<crate::runtime::world::ClientWorld>()
        .stream
        .as_ref()
        .and_then(|stream| stream.authority().actor_rig(100))
        .map(|rig| rig.completed_tick)
}

/// Advances the real clock by one 60 Hz frame and runs the actor publication systems.
fn actor_frame(world: &mut World, clock: &mut Instant) {
    *clock += Duration::from_nanos(16_666_667);
    world
        .resource_mut::<Time<Real>>()
        .update_with_instant(*clock);
    world
        .run_system_cached(crate::runtime::network::advance_actor_frame)
        .unwrap();
    world.run_system_cached(prepare_actor_render_frame).unwrap();
    world.run_system_cached(publish_actor_render_frame).unwrap();
}

fn drawn_actors(world: &World) -> u64 {
    world.resource::<ActorRenderFrame>().rig.instances.len() as u64
}

/// The most allocations any frame without an animation tick made, once `actors` animated
/// actors stand in view.
fn steady_frame_allocations(actors: u64) -> u64 {
    let mut world = steady_actor_world(actors);
    let mut clock = Instant::now();
    let mut worst = 0;
    for frame in 0..60 {
        let before_tick = rig_tick(&world);
        let before = test_alloc::thread_allocations();
        actor_frame(&mut world, &mut clock);
        let allocated = test_alloc::thread_allocations() - before;
        if frame >= 30 && rig_tick(&world) == before_tick {
            worst = worst.max(allocated);
        }
    }
    assert_eq!(drawn_actors(&world), actors, "every actor stands in view");
    worst
}

/// Thirty settled 60 Hz frames of 48 animated actors, ticks included.
#[test]
fn instruction_budget_steady_actor_frames() {
    let mut world = steady_actor_world(48);
    let mut clock = Instant::now();
    for _ in 0..30 {
        actor_frame(&mut world, &mut clock);
    }
    test_instructions::count(|| {
        for _ in 0..30 {
            actor_frame(&mut world, &mut clock);
        }
    });
    assert_eq!(drawn_actors(&world), 48, "every actor stands in view");
}

/// Converted poses, layer poses, bone matrices and build buffers persist across the frames of a
/// tick, so a frame without a tick allocates a bounded amount however many actors it draws.
#[test]
fn steady_actor_frames_do_not_allocate_per_actor() {
    let few = steady_frame_allocations(4);
    let many = steady_frame_allocations(48);
    // Only buffer growth may differ; one allocation per actor would add 44.
    assert!(
        many <= few + 8,
        "4 actors: {few} allocations, 48 actors: {many}"
    );
    assert!(many <= 24, "48 actors: {many} allocations");
}
