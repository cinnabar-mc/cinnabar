//! Entity storage retains its owner through the production scene adapter.

use super::attack_hit_position_tests::{move_actor, unpublished_attack_fixture};
use super::*;
use client_interaction::movement::LocalPhysicsController;

/// Opens entity storage with a live player position for distance checks.
fn open_storage(world: &mut World, owner: i64) {
    world.resource_scope(|world, mut player: Mut<client_state::PlayerRuntime>| {
        let mut ui = world.resource_mut::<UiRuntimeResource>();
        ui.publish_local_runtime_id(&mut player, 7, 42).unwrap();
        ui.publish_inventory_authority(&mut player, protocol::InventoryAuthority::Server);
        ui.enqueue_inventory_event(
            &mut player,
            7,
            1,
            InventoryEvent::Open(protocol::ContainerOpenEvent {
                container: ContainerIdentity::window(7),
                window_type: protocol::WINDOW_TYPE_CONTAINER,
                position: [0; 3],
                runtime_entity_id: owner,
            }),
        )
        .unwrap();
        ui.drain_pending_inventory(&mut player);
        assert!(ui.inventory_open());
    });
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([4.5, 2.620_01, 8.5], 101, false);
    world.insert_resource(physics);
}

/// Commits a chest boat beside the player independently of the attacked actor.
fn spawn_container_owner(world: &mut World) {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let sequence = stream.committed_sequence() + 1;
    stream
        .submit(
            sequence,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id: 100,
                runtime_id: 100,
                kind: protocol::ActorKind::Entity {
                    identifier: "minecraft:chest_boat".into(),
                },
                position: [4.5, 1.5, 8.5],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: protocol::NetworkItemStack::empty(),
                metadata: [].into(),
                attributes: [].into(),
                properties: [].into(),
                links: [].into(),
            })),
        )
        .unwrap();
    test_time::eventually("storage owner spawn", || {
        stream.poll([4.5, 2.620_01, 8.5], 0);
        stream.committed_sequence() >= sequence
    });
    assert!(
        stream
            .authority()
            .inventory_ui()
            .container_actor(100)
            .is_some()
    );
}

#[test]
fn resolved_storage_owner_is_not_replaced_by_an_unrelated_recent_hit() {
    let (mut world, _captured) = unpublished_attack_fixture(7.5);
    spawn_container_owner(&mut world);
    let now = world.resource::<Time<Real>>().elapsed();
    world
        .resource_mut::<BlockUseRuntime>()
        .record_actor_use(7, 99, now);
    open_storage(&mut world, 100);
    world
        .run_system_cached(crate::ui_runtime::scene_stack::close_scenes_on_player_hurt)
        .unwrap();
    move_actor(&mut world, [Some(64.5), None, None], 3);
    world
        .run_system_cached(crate::ui_runtime::scene_stack::close_scenes_on_player_hurt)
        .unwrap();
    assert!(world.resource::<UiRuntimeResource>().inventory_open());

    world
        .resource_mut::<LocalPhysicsController>()
        .reanchor_network_position([64.5, 2.620_01, 8.5], 102, false);
    world
        .run_system_cached(crate::ui_runtime::scene_stack::close_scenes_on_player_hurt)
        .unwrap();
    assert!(!world.resource::<UiRuntimeResource>().inventory_open());
}

#[test]
fn placeholder_storage_owner_retains_the_interacted_actor_after_the_cache_expires() {
    for owner in [0, 1] {
        let (mut world, _captured) = unpublished_attack_fixture(7.5);
        let now = world.resource::<Time<Real>>().elapsed();
        world
            .resource_mut::<BlockUseRuntime>()
            .record_actor_use(7, 99, now);
        open_storage(&mut world, owner);
        world
            .run_system_cached(crate::ui_runtime::scene_stack::close_scenes_on_player_hurt)
            .unwrap();
        assert!(world.resource::<UiRuntimeResource>().inventory_open());

        world
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_secs(6));
        move_actor(&mut world, [Some(64.5), None, None], 3);
        world
            .run_system_cached(crate::ui_runtime::scene_stack::close_scenes_on_player_hurt)
            .unwrap();
        assert!(!world.resource::<UiRuntimeResource>().inventory_open());
    }
}
