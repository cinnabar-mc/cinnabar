//! Seed items plant each air cell once and repeat like any held item use.
use super::*;
use client_ui::ui_runtime::item_facts::SessionItemComponents;
use protocol::ItemUseTrigger;

const SEEDS: i32 = 15000;
const USE_BUTTON: u8 = 2;
const FRAME: Duration = Duration::from_millis(50);
const EYE: [f32; 3] = [4.5, 2.620_01, 8.32];
/// The first floor cell the fixture's camera ray lands on, and the crop cell above it.
const FIRST_SOIL: [i32; 3] = [4, 0, 7];
const FIRST_CROP: [i32; 3] = [4, 1, 7];
/// A second soil cell whose top a retargeted ray reaches without crossing the first crop.
const SECOND_SOIL: [i32; 3] = [5, 0, 8];
const SECOND_CROP: [i32; 3] = [5, 1, 8];

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

/// Turns the fixture floor into farmland and holds 64 seeds that plant `crop` on its top.
fn planting_fixture(crop: &str) -> (World, client_session::CapturedPackets) {
    let (mut world, captured) = floor_fixture(EYE, Quat::from_rotation_x(-0.9601));
    let farmland = default_state("minecraft:farmland");
    let identifier = "fixture:seeds";
    let registry = protocol::ItemRegistryEvent {
        entries: [protocol::ItemRegistryEntry {
            identifier: identifier.into(),
            network_id: SEEDS,
            component_based: true,
            version: protocol::ItemRegistryVersion::DataDriven,
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(64),
            canonical_empty_component_data: false,
            item_tags: [].into(),
            components: None,
        }]
        .into(),
    };
    {
        let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        assert!(stream.seed_item_registry(registry.clone()));
        let sequence = stream.committed_sequence() + 1;
        let soil = (3..=5)
            .flat_map(|x| (6..=9).map(move |z| [x, 0, z]))
            .map(|position| protocol::BlockUpdateEvent {
                dimension: 0,
                position,
                layer: 0,
                network_id: farmland,
            })
            .collect();
        stream
            .submit(sequence, protocol::WorldEvent::BlockUpdates(soil))
            .unwrap();
        test_time::eventually("farmland commit", || {
            stream.poll(EYE, 0);
            stream.committed_sequence() >= sequence
        });
    }
    let components = protocol::ItemComponents {
        seed: Some(std::sync::Arc::new(protocol::ItemSeed {
            crop_result: crop.into(),
            plant_at: [std::sync::Arc::from("minecraft:farmland")].into(),
            plant_at_any_solid_surface: false,
            plant_at_face: 1,
        })),
        ..Default::default()
    };
    world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
        ui.set_session_items(Some(std::sync::Arc::new(SessionItemComponents::from_iter(
            [(std::sync::Arc::from(identifier), components)],
        ))));
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        let mut stack = player.selected_stack().unwrap().clone();
        stack.network_id = SEEDS;
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

/// Reads the primary block the local client currently shows at `position`.
fn cell(world: &World, position: [i32; 3]) -> u32 {
    let client = world.resource::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_ref().unwrap();
    sim::PaletteWorld::new(
        stream.collision_store(),
        world
            .resource::<PhysicsCollisionRegistries>()
            .registry(stream.network_id_mode()),
        stream.current_dimension(),
        Default::default(),
    )
    .primary_runtime_id(position)
    .unwrap()
}

/// The held count after every locally committed use.
fn held_count(world: &World) -> u16 {
    let player = world.resource::<client_state::PlayerRuntime>();
    let ui = world.resource::<UiRuntimeResource>();
    let selection = verified_use_selection(player, ui).unwrap();
    let revision = ui
        .inventory_ledger(player)
        .authoritative_slot_revision(selection.slot)
        .unwrap_or(0);
    world
        .resource::<BlockUseRuntime>()
        .inventory
        .predicted_selection(&selection, revision)
        .unwrap_or(selection)
        .item
        .count()
}

/// What the server received during one frame: swings and item-use transactions.
#[derive(Debug, Default, PartialEq)]
struct Sent {
    swings: usize,
    uses: Vec<(Option<ItemUseTrigger>, [i32; 3], bool)>,
}

/// Runs one 50 ms frame with Use held or released and returns what it sent.
fn step(
    world: &mut World,
    router: &mut crate::semantic_controls::SemanticInputRuntime,
    captured: &mut client_session::CapturedPackets,
    held: bool,
) -> Sent {
    let buttons = if held { vec![USE_BUTTON] } else { Vec::new() };
    hold_fixed_ray(world, router, Vec::new(), buttons);
    world.resource_mut::<Time<Real>>().advance_by(FRAME);
    world.run_system_cached(produce_block_use).unwrap();
    let packets = captured.drain();
    world
        .run_system_cached(crate::movement::advance_local_physics)
        .unwrap();
    Sent {
        swings: packets
            .iter()
            .filter(|packet| format!("{:?}", packet.kind()) == "AnimatePacket")
            .count(),
        uses: packets
            .iter()
            .filter_map(item_use)
            .map(|tx| (tx.trigger, tx.position, tx.predicted_success))
            .collect(),
    }
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

/// The place cues written so far, as the cell each one sounds at.
fn place_cue_cells(world: &World) -> Vec<[i32; 3]> {
    world
        .resource::<Messages<client_audio::audio::LocalBlockCue>>()
        .iter_current_update_messages()
        .filter_map(|cue| match *cue {
            client_audio::audio::LocalBlockCue::Place { position, .. } => Some(position),
            client_audio::audio::LocalBlockCue::Break { .. } => None,
        })
        .collect()
}

/// A second press before the server's block update finds the predicted crop in the cell, so
/// the server sees one planting and the stack loses one item.
#[test]
fn a_double_press_before_the_block_update_plants_and_consumes_once() {
    let (mut world, mut captured) = planting_fixture("minecraft:wheat");
    let wheat = default_state("minecraft:wheat");
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    settle(&mut world, &mut router);

    let press = step(&mut world, &mut router, &mut captured, true);
    assert_eq!(
        press,
        Sent {
            swings: 1,
            uses: vec![(Some(ItemUseTrigger::PlayerInput), FIRST_SOIL, true)],
        }
    );
    assert_eq!(cell(&world, FIRST_CROP), wheat);
    assert_eq!(held_count(&world), 63);
    assert_eq!(
        place_cue_cells(&world),
        [FIRST_SOIL],
        "the local sound must share the clicked cell with the server's copy"
    );

    let release = step(&mut world, &mut router, &mut captured, false);
    assert_eq!(release, Sent::default());
    let repress = step(&mut world, &mut router, &mut captured, true);
    assert_eq!(repress.swings, 0, "a refused planting does not swing");
    assert!(
        repress
            .uses
            .iter()
            .all(|(_, _, predicted_success)| !predicted_success),
        "the second press predicts nothing: {repress:?}"
    );
    assert_eq!(cell(&world, FIRST_CROP), wheat);
    assert_eq!(held_count(&world), 63);
    assert_eq!(place_cue_cells(&world), [FIRST_SOIL]);
}

/// Holding Use plants the next soil cell the ray reaches after the repeat delay, then stops
/// once that cell holds a crop.
#[test]
fn holding_use_plants_the_next_cell_on_a_simulation_tick() {
    let (mut world, mut captured) = planting_fixture("minecraft:wheat");
    let wheat = default_state("minecraft:wheat");
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    settle(&mut world, &mut router);

    let press = step(&mut world, &mut router, &mut captured, true);
    assert_eq!(
        press.uses,
        [(Some(ItemUseTrigger::PlayerInput), FIRST_SOIL, true)]
    );

    let eye = Vec3::from_array(EYE);
    let target = Vec3::new(5.5, 1.0, 8.5);
    let origin = frame_origin(
        &world,
        eye,
        Quat::from_rotation_arc(Vec3::NEG_Z, (target - eye).normalize()),
    );
    retain_pick(&mut world, &origin);

    let mut planted = Vec::new();
    let mut swings = 0;
    for _ in 0..10 {
        let sent = step(&mut world, &mut router, &mut captured, true);
        swings += sent.swings;
        planted.extend(sent.uses.into_iter().filter(|(_, _, success)| *success));
    }
    assert_eq!(
        planted,
        [(Some(ItemUseTrigger::SimulationTick), SECOND_SOIL, true)]
    );
    assert_eq!(swings, 1);
    assert_eq!(cell(&world, SECOND_CROP), wheat);
    assert_eq!(held_count(&world), 62);
}

/// A crop defined by the session's block palette plants its first state like a vanilla crop.
#[test]
fn a_session_defined_crop_plants_its_first_palette_state() {
    let (mut world, mut captured) = planting_fixture("fixture:crop");
    let mut custom =
        protocol::CustomBlocks::from_definitions([("fixture:crop", [10, 0, 0].as_slice())]);
    custom.vanilla_blocks = assets::server_defined_blocks()
        .iter()
        .map(|block| block.name.into())
        .collect();
    assert_eq!(custom.blocks.len(), 1);
    let first = world
        .resource_mut::<PhysicsCollisionRegistries>()
        .begin_session_custom_blocks(&custom)
        .unwrap()
        .0
        .start;
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    settle(&mut world, &mut router);

    let press = step(&mut world, &mut router, &mut captured, true);
    assert_eq!(
        press.uses,
        [(Some(ItemUseTrigger::PlayerInput), FIRST_SOIL, true)]
    );
    assert_eq!(press.swings, 1);
    assert_eq!(cell(&world, FIRST_CROP), first);
    assert_eq!(held_count(&world), 63);
}
