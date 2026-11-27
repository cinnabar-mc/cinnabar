//! A held book yields a use press to an interactive block in front of it.
use super::*;
use crate::ui_runtime::interaction::inventory::drive_world_inventory_keys;
use protocol::{InventoryContentEvent, ItemRegistryEvent};

/// The aimed fixture cell, which these tests turn into an interactive block.
const AIMED: [i32; 3] = [4, 2, 6];

/// Replaces the aimed block with a lectern and retakes the frame pick.
fn aim_at_lectern(world: &mut World) {
    aim_at(world, "minecraft:lectern");
}

/// Replaces the aimed block with `name`'s first state and retakes the frame pick.
fn aim_at(world: &mut World, name: &str) {
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let block = records
        .iter()
        .find(|record| record.name.as_ref() == name)
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
                network_id: block,
            }]),
        )
        .unwrap();
    test_time::eventually("block update decode", || {
        stream.poll([4.5, 2.620_01, 8.5], 0);
        stream.committed_sequence() >= sequence
    });
    world
        .run_system_cached(crate::block_use::retain_block_use_pick)
        .unwrap();
}

/// Selects a fresh writable book and presses the mouse's Use button.
fn press_use_with_book(world: &mut World) {
    select_book(world);
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
}

/// Selects a fresh writable book, with every button released.
fn select_book(world: &mut World) {
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

/// These blocks' own use fails for a book, but vanilla still gives them the press, so the book
/// stays closed and its air use never runs.
#[test]
fn a_held_book_leaves_the_use_press_to_blocks_whose_use_fails() {
    for name in [
        "minecraft:jukebox",
        "minecraft:cauldron",
        "minecraft:composter",
    ] {
        let (mut world, mut captured) = fixture();
        aim_at(&mut world, name);
        press_use_with_book(&mut world);
        world.run_system_cached(drive_world_inventory_keys).unwrap();
        assert!(!book_open(&world), "the {name} takes the press");
        world.run_system_cached(produce_block_use).unwrap();
        assert_eq!(transaction_targets(&mut captured), [AIMED], "{name}");
        assert!(
            world.resource::<BlockUseRuntime>().press_consumed(),
            "the {name} keeps the press from the item's air use"
        );
    }
}

#[test]
fn a_held_book_opens_when_no_block_takes_the_press() {
    let (mut world, _captured) = fixture();
    aim_at_lectern(&mut world);
    look_up(&mut world);
    press_use_with_book(&mut world);
    world.run_system_cached(drive_world_inventory_keys).unwrap();
    assert!(book_open(&world));
}

/// Points the fixture's view straight up, so no block in front takes a use press.
fn look_up(world: &mut World) {
    let looking_up = frame_origin(
        world,
        Vec3::new(4.5, 2.5, 8.5),
        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
    );
    world.insert_resource(looking_up);
    world
        .run_system_cached(crate::block_use::retain_block_use_pick)
        .unwrap();
}

/// A world tap presses Use a few frames after its finger lifts, once the pick has caught up
/// with its ray, so the press can land on a frame that carries no raw input at all. The
/// production registration still lets the book reader see it.
#[test]
fn a_deferred_touch_use_opens_the_held_book_on_a_frame_without_raw_input() {
    use bevy::{
        ecs::{message::Messages, schedule::Schedule},
        input::{
            keyboard::{KeyboardFocusLost, KeyboardInput},
            touch::Touches,
        },
    };
    let (mut world, _captured) = fixture();
    look_up(&mut world);
    select_book(&mut world);
    // Every raw input source is present and quiet: no button edges, buffered keyboard or
    // focus messages, touches or gamepads.
    world.init_resource::<Touches>();
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<KeyboardFocusLost>>();
    let mut touch = crate::semantic_controls::SemanticTouchTargets::default();
    touch.press(semantic_input::touch::USE);
    world.insert_resource(touch);
    let mut schedule = Schedule::default();
    schedule
        .configure_sets(
            client_shell::input_activity::RawInputReaders
                .run_if(client_shell::input_activity::raw_input_activity),
        )
        .add_systems(crate::app::authority::world_inventory_keys());
    schedule.run(&mut world);
    assert!(book_open(&world), "the deferred tap's Use opens the book");
}
