//! Bucket targets through the production interaction producer and negotiated item registry.

use super::*;
use assets::facts::{EMPTY_BUCKET_IDENTIFIER, FROG_SPAWN_IDENTIFIER};

/// Replays the server's block runtime id on the held block item descriptor.
fn set_held_block_runtime_id(world: &mut World, block_runtime_id: u32) {
    world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        let mut stack = player.selected_stack().unwrap().clone();
        stack.block_runtime_id = i32::try_from(block_runtime_id).unwrap();
        ui.inventory_ledger_mut(&mut player)
            .apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity {
                        window_id: Some(0),
                        slot_type: None,
                        dynamic_id: None,
                    },
                    slot: 0,
                },
                stack,
                storage_item: None,
            }));
    });
}

/// Replaces the chest with a liquid and puts an ordinary block behind it.
fn liquid_fixture(
    identifier: &str,
    metadata: u32,
    liquid: &str,
    depth: u8,
    obstructed: bool,
) -> (World, client_session::CapturedPackets) {
    let (mut world, captured) = fixture();
    item_use_tests::hold_air_item(&mut world, identifier, 1, std::sync::Arc::from([]));
    if metadata != 0 {
        world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
            let mut player = world.resource_mut::<client_state::PlayerRuntime>();
            let mut stack = player.selected_stack().unwrap().clone();
            stack.metadata = metadata;
            ui.inventory_ledger_mut(&mut player)
                .apply(&InventoryEvent::Slot(InventorySlotEvent {
                    identity: SlotIdentity {
                        container: ContainerIdentity {
                            window_id: Some(0),
                            slot_type: None,
                            dynamic_id: None,
                        },
                        slot: 0,
                    },
                    stack,
                    storage_item: None,
                }));
        });
    }
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let liquid = records
        .iter()
        .find(|record| {
            record.name.as_ref() == liquid
                && serde_json::from_str::<serde_json::Value>(&record.canonical_state).unwrap()
                    [assets::facts::BlockProperty::LiquidDepth.name()]["value"]
                    .as_u64()
                    == Some(u64::from(depth))
        })
        .unwrap()
        .sequential_id;
    let stone = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:stone")
        .unwrap()
        .sequential_id;
    if identifier == FROG_SPAWN_IDENTIFIER {
        set_held_block_runtime_id(
            &mut world,
            records
                .iter()
                .find(|record| record.name.as_ref() == FROG_SPAWN_IDENTIFIER)
                .unwrap()
                .sequential_id,
        );
    }
    let mut updates = vec![
        protocol::BlockUpdateEvent {
            dimension: 0,
            position: [4, 2, 6],
            layer: 0,
            network_id: liquid,
        },
        protocol::BlockUpdateEvent {
            dimension: 0,
            position: [4, 2, 5],
            layer: 0,
            network_id: stone,
        },
    ];
    if obstructed {
        updates.push(protocol::BlockUpdateEvent {
            dimension: 0,
            position: [4, 2, 7],
            layer: 0,
            network_id: stone,
        });
    }
    {
        let mut client_world = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client_world.stream.as_mut().unwrap();
        stream
            .submit(3, protocol::WorldEvent::BlockUpdates(updates))
            .unwrap();
        test_time::eventually("liquid fixture commit", || {
            stream.poll([4.5, 2.620_01, 8.5], 0);
            stream.committed_sequence() >= 3
        });
    }
    world.resource_mut::<BlockUseRuntime>().clear_picks();
    world
        .run_system_cached(crate::block_use::retain_block_use_pick)
        .unwrap();
    (world, captured)
}

#[test]
fn empty_bucket_clicks_the_liquid_source_instead_of_the_block_behind_it() {
    assert!(
        protocol::vanilla_item_registry()
            .iter()
            .any(|entry| entry.identifier.as_ref() == EMPTY_BUCKET_IDENTIFIER)
    );
    for liquid in ["minecraft:water", "minecraft:lava"] {
        let (mut world, mut captured) =
            liquid_fixture(EMPTY_BUCKET_IDENTIFIER, 0, liquid, 0, false);
        world.run_system_cached(produce_block_use).unwrap();
        assert_eq!(transaction_targets(&mut captured), [[4, 2, 6]], "{liquid}");
        let player = world.resource::<client_state::PlayerRuntime>();
        assert_eq!(player.selected_stack().unwrap().metadata, 0);
        assert_eq!(player.selected_stack().unwrap().count, 1);
    }
}

#[test]
fn frogspawn_uses_the_liquid_aware_pick_for_water_sources() {
    let (mut world, mut captured) =
        liquid_fixture(FROG_SPAWN_IDENTIFIER, 0, "minecraft:water", 0, false);
    let eye = Vec3::new(4.5, 4.0, 8.5);
    let rotation = Quat::from_rotation_x(-0.42);
    let origin = frame_origin(&world, eye, rotation);
    world.resource_mut::<BlockUseRuntime>().clear_picks();
    retain_pick(&mut world, &origin);
    world.insert_resource(origin);
    world.insert_resource(LocalViewPose::new(eye, rotation));
    let pick = world
        .resource::<BlockUseRuntime>()
        .pick(world.resource::<MovementTicker>())
        .expect("frogspawn should retain a frame pick");
    assert!(matches!(
        pick.hits.liquid,
        Some(gameplay::block_use::PickResult::Block(hit)) if hit.position == [4, 2, 6]
    ));
    world.run_system_cached(produce_block_use).unwrap();
    let uses = captured
        .drain()
        .iter()
        .filter_map(item_use)
        .collect::<Vec<_>>();
    assert_eq!(uses.len(), 1);
    assert!(uses[0].predicted_success);
    // The transaction keeps the clicked water position; the placement
    // destination sent in the preceding start-use packet is the cell above.
    assert_eq!(uses[0].position, [4, 2, 6]);
    assert_eq!(uses[0].face, 1);
    assert_eq!(
        world
            .resource::<BlockUseRuntime>()
            .last_success_destination(),
        Some([4, 3, 6])
    );
}

#[test]
fn liquid_pick_keeps_flowing_liquids_other_items_and_solid_occluders_distinct() {
    for (item, metadata, depth, obstructed, expected) in [
        (EMPTY_BUCKET_IDENTIFIER, 0, 1, false, [4, 2, 5]),
        (EMPTY_BUCKET_IDENTIFIER, 8, 0, false, [4, 2, 5]),
        ("minecraft:water_bucket", 0, 0, false, [4, 2, 5]),
        ("minecraft:diamond_sword", 0, 0, false, [4, 2, 5]),
        ("fixture:bucket", 0, 0, false, [4, 2, 5]),
        (EMPTY_BUCKET_IDENTIFIER, 0, 0, true, [4, 2, 7]),
    ] {
        let (mut world, mut captured) =
            liquid_fixture(item, metadata, "minecraft:water", depth, obstructed);
        world.run_system_cached(produce_block_use).unwrap();
        assert_eq!(
            transaction_targets(&mut captured),
            [expected],
            "{item} {depth} {obstructed}"
        );
    }
}

/// A streaming gap beyond a loaded source cannot erase the independently available liquid pick.
#[test]
fn a_valid_liquid_pick_survives_an_unavailable_ordinary_query() {
    let (mut world, mut captured) =
        liquid_fixture(EMPTY_BUCKET_IDENTIFIER, 0, "minecraft:water", 0, false);
    let eye = [4.5, 2.5, 11.5];
    {
        let registry = world.resource::<PhysicsCollisionRegistries>();
        let client = world.resource::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_ref().unwrap();
        let source = PaletteWorld::new(
            stream.collision_store(),
            registry.registry(stream.network_id_mode()),
            0,
            Default::default(),
        )
        .primary_runtime_id([4, 2, 6])
        .unwrap();
        let _ = (registry, client);
        let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        let sequence = stream.committed_sequence() + 1;
        stream
            .submit(
                sequence,
                protocol::WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
                    dimension: 0,
                    position: [4, 2, 12],
                    layer: 0,
                    network_id: source,
                }]),
            )
            .unwrap();
        test_time::eventually("stream-boundary liquid source", || {
            stream.poll(eye, 0);
            stream.committed_sequence() >= sequence
        });
    }
    {
        let mut movement = world.resource_mut::<MovementTicker>();
        movement.reset(7, 100, eye);
        movement.set_source(gameplay::movement::MovementSource::Physics);
        let mut sample = gameplay::test_support::survival_mining::completed(101);
        sample.position = eye;
        movement.enqueue_completed_physics(sample).unwrap();
    }
    world.resource_mut::<BlockUseRuntime>().clear_picks();
    let origin = frame_origin(
        &world,
        Vec3::from_array(eye),
        Quat::from_rotation_y(std::f32::consts::PI),
    );
    {
        let client = world.resource::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_ref().unwrap();
        let registry = world.resource::<PhysicsCollisionRegistries>();
        let ray = origin.outbound_ray().unwrap();
        let vector = |v: Vec3| sim::Vec3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z));
        for liquids in [false, true] {
            let query = PaletteWorld::new(
                stream.collision_store(),
                registry.registry(stream.network_id_mode()),
                0,
                assets::block_shapes::PickContext {
                    liquids,
                    ..Default::default()
                },
            )
            .block_interaction_ray_current(
                vector(ray.origin()),
                vector(ray.direction()),
                gameplay::mining::survival_reach(PlayerInputMode::Mouse),
            );
            if liquids {
                assert_eq!(
                    query
                        .expect("liquid source is fully known")
                        .unwrap()
                        .block_pos,
                    [4, 2, 12]
                );
            } else {
                assert!(
                    query.is_err(),
                    "ordinary ray must encounter the later streaming gap"
                );
            }
        }
    }
    world.insert_resource(origin);
    world
        .run_system_cached(crate::block_use::retain_block_use_pick)
        .unwrap();
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(transaction_targets(&mut captured), [[4, 2, 12]]);
}
