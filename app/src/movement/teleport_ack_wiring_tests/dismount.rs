use super::*;

/// The local player riding a boat on the fixture's stone row.
fn riding_world() -> ClientWorld {
    let mut stream = super::correction_presentation::grounded_stream([4.5, 2.62, 8.5]);
    stream
        .submit(
            3,
            WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id: 77,
                runtime_id: 77,
                kind: protocol::ActorKind::Entity {
                    identifier: "minecraft:boat".into(),
                },
                position: [5.5, 1.0, 8.5],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: std::sync::Arc::from([]),
                attributes: std::sync::Arc::from([]),
                properties: std::sync::Arc::from([]),
                links: std::sync::Arc::from([]),
            })),
        )
        .unwrap();
    stream
        .submit(
            4,
            WorldEvent::ActorLink(protocol::ActorLinkEvent {
                dimension: 0,
                ridden_unique_id: 77,
                rider_unique_id: 1,
                link_type: protocol::ActorLinkType::Rider,
                immediate: false,
                rider_initiated: false,
            }),
        )
        .unwrap();
    ClientWorld {
        stream: Some(stream),
        ..ClientWorld::default()
    }
}

/// Collision boxes around the boat in `world`.
fn boxes_at_the_boat(world: &impl sim::CollisionWorld) -> usize {
    let around = sim::Aabb::new(sim::Vec3::new(4.0, 0.5, 7.5), sim::Vec3::new(7.0, 2.0, 9.5));
    world.collision_boxes(around).unwrap().value.len()
}

/// The frame's ticks after the player gets off run in a collision view that holds the boat
/// it left, so walking back into it stops at its side from the first tick.
#[test]
fn the_ticks_after_getting_off_collide_with_the_mount_left() {
    let registries = fixture_registries();
    let mut client_world = riding_world();
    let stream = client_world.stream.as_ref().unwrap();
    assert!(stream.authority().local_mount_unique_id() == Some(77));
    let seated = boxes_at_the_boat(&super::super::collision_world(
        stream,
        registries.registry(stream.network_id_mode()),
        &client_world.movement_colliders,
    ));
    let world = super::super::runtime_system::leave_local_mount(&mut client_world, &registries)
        .expect("the stream stays connected");
    assert_eq!(boxes_at_the_boat(&world), seated + 1, "the boat collides");
}
