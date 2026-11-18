use super::*;
use std::sync::Arc;

/// Supplies an empty authority for pick targets.
pub(super) fn world() -> client_world::WorldAuthority {
    client_world::WorldAuthority::new(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 999,
            local_player_unique_id: 999,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    )
}

#[test]
fn native_painting_pick_resolves_unique_id_and_respects_walls_and_removal() {
    let mut world = world();
    let event = protocol::test_support::packets::add_painting_event(
        -73,
        91,
        "backyard",
        2,
        [0.0, 70.5, 3.5],
    )
    .unwrap()
    .unwrap();
    world.apply_ordered_event(event, Some(1)).unwrap();
    for block in [None, Some(4.0)] {
        assert_eq!(
            actor_pick_unique_id(&world, [0.0, 70.5, 0.0], [0.0, 0.0, 1.0], 5.0, block, None),
            Some(-73)
        );
    }
    assert_eq!(
        actor_pick_unique_id(
            &world,
            [0.0, 70.5, 0.0],
            [0.0, 0.0, 1.0],
            5.0,
            Some(2.0),
            None
        ),
        None
    );
    assert_eq!(
        actor_pick_unique_id(&world, [2.0, 70.5, 0.0], [0.0, 0.0, 1.0], 5.0, None, None),
        None
    );
    assert_eq!(
        actor_pick_unique_id(
            &world,
            [0.0, 70.5, 0.0],
            [0.0, 0.0, 1.0],
            5.0,
            None,
            Some(-73)
        ),
        None
    );
    world
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::Remove(protocol::ActorRemoveEvent {
                dimension: 0,
                unique_id: -73,
            })),
            Some(2),
        )
        .unwrap();
    assert_eq!(
        actor_pick_unique_id(&world, [0.0, 70.5, 0.0], [0.0, 0.0, 1.0], 5.0, None, None),
        None
    );
}

#[test]
fn mob_pick_resolves_the_unique_id_instead_of_the_backing_block() {
    for identifier in ["minecraft:cow", "minecraft:villager_v2"] {
        let mut world = world();
        world
            .apply_ordered_event(
                protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(
                    protocol::ActorSpawnEvent {
                        dimension: 0,
                        unique_id: -73,
                        runtime_id: 91,
                        kind: protocol::ActorKind::Entity {
                            identifier: identifier.into(),
                        },
                        position: [0.0, 70.0, 3.0],
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
                    },
                )),
                Some(1),
            )
            .unwrap();
        for block_distance in [None, Some(4.0)] {
            assert_eq!(
                actor_pick_unique_id(
                    &world,
                    [0.0, 70.5, 0.0],
                    [0.0, 0.0, 1.0],
                    5.0,
                    block_distance,
                    None
                ),
                Some(-73),
                "{identifier}"
            );
        }
        for (reach, block_distance, excluded) in [
            (5.0, Some(1.0), None),
            (1.0, None, None),
            (5.0, None, Some(-73)),
        ] {
            assert_eq!(
                actor_pick_unique_id(
                    &world,
                    [0.0, 70.5, 0.0],
                    [0.0, 0.0, 1.0],
                    reach,
                    block_distance,
                    excluded
                ),
                None,
                "{identifier}"
            );
        }
    }
}

/// An actor riding the local player is never its pick target, so a carried passenger does not
/// take the rays meant for blocks, items and other actors.
#[test]
fn actors_riding_the_local_player_are_not_pick_candidates() {
    let mut world = world();
    let event = protocol::test_support::packets::add_painting_event(
        -73,
        91,
        "backyard",
        2,
        [0.0, 70.5, 3.5],
    )
    .unwrap()
    .unwrap();
    world.apply_ordered_event(event, Some(1)).unwrap();
    let link = |link_type| {
        protocol::WorldEvent::ActorLink(protocol::ActorLinkEvent {
            dimension: 0,
            ridden_unique_id: 999,
            rider_unique_id: -73,
            link_type,
            immediate: false,
            rider_initiated: false,
        })
    };
    assert_eq!(world.pick_candidates().count(), 1);
    world
        .apply_ordered_event(link(protocol::ActorLinkType::Passenger), Some(2))
        .unwrap();
    assert!(world.rides_local_player(-73));
    assert_eq!(world.pick_candidates().count(), 0);
    assert_eq!(world.remote_actors().count(), 1);
    world
        .apply_ordered_event(link(protocol::ActorLinkType::Remove), Some(3))
        .unwrap();
    assert_eq!(world.pick_candidates().count(), 1);
}
