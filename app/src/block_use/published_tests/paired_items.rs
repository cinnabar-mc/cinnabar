//! Door and bed items place through their own use, and a held click waits out the repeat delay.
use super::*;
use protocol::ItemUseTrigger;

const ITEM: i32 = 15001;
const USE_BUTTON: u8 = 2;
const FRAME: Duration = Duration::from_millis(50);
const EYE: [f32; 3] = [4.5, 2.620_01, 8.32];
/// The floor cell the fixture's camera ray lands on, and the cell above it.
const FLOOR: [i32; 3] = [4, 0, 7];
const FOOT: [i32; 3] = [4, 1, 7];

/// The first permutation of `name` in the registry the fixture's collisions bind.
fn default_state(name: &str) -> u32 {
    assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
    .iter()
    .filter(|record| record.name.as_ref() == name)
    .map(|record| record.sequential_id)
    .min()
    .unwrap()
}

/// A stone floor and a held item named `identifier` that carries no block of its own.
fn holding(identifier: &str) -> (World, client_session::CapturedPackets) {
    let (mut world, captured) = floor_fixture(EYE, Quat::from_rotation_x(-0.9601));
    let registry = protocol::ItemRegistryEvent {
        entries: [protocol::ItemRegistryEntry {
            identifier: identifier.into(),
            network_id: ITEM,
            component_based: false,
            version: protocol::ItemRegistryVersion::Legacy,
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(64),
            canonical_empty_component_data: false,
            item_tags: [].into(),
            components: None,
        }]
        .into(),
    };
    assert!(
        world
            .resource_mut::<crate::runtime::world::ClientWorld>()
            .stream
            .as_mut()
            .unwrap()
            .seed_item_registry(registry.clone())
    );
    world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        let mut stack = player.selected_stack().unwrap().clone();
        stack.network_id = ITEM;
        stack.block_runtime_id = 0;
        let ledger = ui.inventory_ledger_mut(&mut player);
        ledger.apply_registry(&registry);
        ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
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
    (world, captured)
}

/// Applies the block the server reports at `position`, ahead of the next frame.
fn server_places(world: &mut World, position: [i32; 3], network_id: u32) {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let sequence = stream.committed_sequence() + 1;
    stream
        .submit(
            sequence,
            protocol::WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
                dimension: 0,
                position,
                layer: 0,
                network_id,
            }]),
        )
        .unwrap();
    test_time::eventually("server block update", || {
        stream.poll(EYE, 0);
        stream.committed_sequence() >= sequence
    });
}

/// A use transaction's trigger, clicked cell and predicted result.
type SentUse = (Option<ItemUseTrigger>, [i32; 3], bool);

/// Runs one 50 ms frame with Use held and returns the swings and use transactions it sent.
fn step(
    world: &mut World,
    router: &mut crate::semantic_controls::SemanticInputRuntime,
    captured: &mut client_session::CapturedPackets,
) -> (usize, Vec<SentUse>) {
    hold_fixed_ray(world, router, Vec::new(), vec![USE_BUTTON]);
    world.resource_mut::<Time<Real>>().advance_by(FRAME);
    world.run_system_cached(produce_block_use).unwrap();
    let packets = captured.drain();
    world
        .run_system_cached(crate::movement::advance_local_physics)
        .unwrap();
    (
        packets
            .iter()
            .filter(|packet| format!("{:?}", packet.kind()) == "AnimatePacket")
            .count(),
        packets
            .iter()
            .filter_map(item_use)
            .map(|tx| (tx.trigger, tx.position, tx.predicted_success))
            .collect(),
    )
}

/// Spends the reanchor's discarded first elapsed time so frames simulate ticks.
fn settle(world: &mut World, router: &mut crate::semantic_controls::SemanticInputRuntime) {
    hold_fixed_ray(world, router, Vec::new(), Vec::new());
    world
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::ZERO);
    world
        .run_system_cached(crate::movement::advance_local_physics)
        .unwrap();
}

/// Each closed, unhinged wooden door facing, as its lower and upper half states.
fn door_facings() -> Vec<[u32; 2]> {
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let mut halves = std::collections::BTreeMap::<String, [Option<u32>; 2]>::new();
    for record in records.iter() {
        if record.name.as_ref() != "minecraft:wooden_door" {
            continue;
        }
        let states: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let value = |key: &str| &states[key]["value"];
        let set = |key: &str| {
            let value = value(key);
            value
                .as_i64()
                .map_or_else(|| value.as_bool().unwrap(), |value| value != 0)
        };
        if set("open_bit") || set("door_hinge_bit") {
            continue;
        }
        let upper = set("upper_block_bit");
        let facing = value("minecraft:cardinal_direction").to_string();
        let half = &mut halves.entry(facing).or_default()[usize::from(upper)];
        *half = Some(half.map_or(record.sequential_id, |id| id.min(record.sequential_id)));
    }
    let facings = halves
        .into_values()
        .map(|[lower, upper]| [lower.unwrap(), upper.unwrap()])
        .collect::<Vec<_>>();
    assert_eq!(facings.len(), 4);
    facings
}

/// Presses Use on the floor, has the server place `placed` above it, and holds for two
/// more frames.
fn click_within_the_repeat_delay(item: &str, placed: &[([i32; 3], u32)]) {
    let (mut world, mut captured) = holding(item);
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    settle(&mut world, &mut router);

    let (swings, press) = step(&mut world, &mut router, &mut captured);
    for &(position, block) in placed {
        server_places(&mut world, position, block);
    }
    for frame in 0..2 {
        let (_, uses) = step(&mut world, &mut router, &mut captured);
        assert!(
            uses.is_empty(),
            "frame {frame} of a held {item} click used the block it placed: {uses:?}"
        );
    }
    assert_eq!(swings, 1, "a successful {item} use swings");
    assert_eq!(press, [(Some(ItemUseTrigger::PlayerInput), FLOOR, true)]);
}

/// The repeat waits out native's delay from the press, so a short click leaves the door shut
/// whichever way the server faces it.
#[test]
fn a_held_door_click_does_not_open_the_door_it_placed() {
    for [lower, upper] in door_facings() {
        click_within_the_repeat_delay(
            "minecraft:wooden_door",
            &[(FOOT, lower), ([FOOT[0], FOOT[1] + 1, FOOT[2]], upper)],
        );
    }
}

/// The bed the click placed is not slept in by the same click.
#[test]
fn a_held_bed_click_does_not_use_the_bed_it_placed() {
    click_within_the_repeat_delay("minecraft:bed", &[(FOOT, default_state("minecraft:bed"))]);
}

/// Straw beds share the bed family and its use.
#[test]
fn a_held_straw_bed_click_does_not_use_the_bed_it_placed() {
    click_within_the_repeat_delay(
        "minecraft:straw_bed",
        &[(FOOT, default_state("minecraft:straw_bed"))],
    );
}

/// The door is drawn locally before the server's update, as native places it on the press.
#[test]
fn a_door_press_predicts_both_halves() {
    let (mut world, mut captured) = holding("minecraft:wooden_door");
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    settle(&mut world, &mut router);
    step(&mut world, &mut router, &mut captured);

    let client = world.resource::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_ref().unwrap();
    let collisions = world.resource::<PhysicsCollisionRegistries>();
    let palette = sim::PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(stream.network_id_mode()),
        stream.current_dimension(),
        Default::default(),
    );
    for cell in [FOOT, [FOOT[0], FOOT[1] + 1, FOOT[2]]] {
        let block = palette.primary_runtime_id(cell).unwrap();
        assert_eq!(
            collisions.block_identifier(stream.network_id_mode(), block),
            Some("minecraft:wooden_door")
        );
    }
}

/// A door item that cannot go here keeps the failed-use path: no swing and no placement.
#[test]
fn a_door_press_without_room_does_not_swing() {
    let (mut world, mut captured) = holding("minecraft:wooden_door");
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    settle(&mut world, &mut router);
    server_places(
        &mut world,
        [FOOT[0], FOOT[1] + 1, FOOT[2]],
        default_state("minecraft:stone"),
    );
    let (swings, _) = step(&mut world, &mut router, &mut captured);
    assert_eq!(swings, 0);
}
