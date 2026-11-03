//! A held book yields a use press to an interactive block in front of it.
use super::*;
use crate::ui_runtime::interaction::inventory::drive_world_inventory_keys;
use protocol::{InventoryContentEvent, ItemRegistryEvent};

/// The aimed fixture cell, which these tests turn into a lectern.
const AIMED: [i32; 3] = [4, 2, 6];

/// Replaces the aimed block with a lectern and retakes the frame pick.
fn aim_at_lectern(world: &mut World) {
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let lectern = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:lectern")
        .unwrap()
        .sequential_id;
    let mut client_world = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client_world.stream.as_mut().unwrap();
    let sequence = stream.committed_sequence() + 1;
    stream
        .submit(
            sequence,
            protocol::WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
                dimension: 0,
                position: AIMED,
                layer: 0,
                network_id: lectern,
            }]),
        )
        .unwrap();
    test_time::eventually("lectern update decode", || {
        stream.poll([4.5, 2.620_01, 8.5], 0);
        stream.committed_sequence() >= sequence
    });
    world
        .run_system_cached(crate::block_use::retain_block_use_pick)
        .unwrap();
}

/// Selects a fresh writable book and presses the mouse's Use button.
fn press_use_with_book(world: &mut World) {
    let registry = protocol::vanilla_item_registry();
    let network_id = registry
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:writable_book")
        .unwrap()
        .network_id;
    let mut player = world.resource_mut::<client_state::PlayerRuntime>();
    let ledger = player.inventory.ledger_mut();
    ledger.apply_registry(&ItemRegistryEvent { entries: registry });
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
        slots: vec![protocol::NetworkItemStack {
            network_id,
            count: 1,
            stack_network_id: 9,
            ..protocol::NetworkItemStack::empty()
        }]
        .into(),
        storage_item: protocol::NetworkItemStack::empty(),
    }));
    world.init_resource::<ButtonInput<KeyCode>>();
    world.init_resource::<ButtonInput<MouseButton>>();
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
}

/// Whether the book screen is open.
fn book_open(world: &World) -> bool {
    world
        .resource::<UiRuntimeResource>()
        .screen_state()
        .book
        .is_some()
}

#[test]
fn a_held_book_leaves_the_use_press_to_a_lectern() {
    let (mut world, mut captured) = fixture();
    aim_at_lectern(&mut world);
    press_use_with_book(&mut world);
    world.run_system_cached(drive_world_inventory_keys).unwrap();
    assert!(!book_open(&world), "the lectern takes the press");
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(transaction_targets(&mut captured), [AIMED]);
    assert!(world.resource::<BlockUseRuntime>().press_consumed());
}

#[test]
fn a_held_book_opens_when_no_block_takes_the_press() {
    let (mut world, _captured) = fixture();
    aim_at_lectern(&mut world);
    let looking_up = frame_origin(
        &world,
        Vec3::new(4.5, 2.5, 8.5),
        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
    );
    world.insert_resource(looking_up);
    world
        .run_system_cached(crate::block_use::retain_block_use_pick)
        .unwrap();
    press_use_with_book(&mut world);
    world.run_system_cached(drive_world_inventory_keys).unwrap();
    assert!(book_open(&world));
}
