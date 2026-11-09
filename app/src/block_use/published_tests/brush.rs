//! A brush used on a block keeps brushing while Use is held and releases with the button.
use super::*;
use protocol::ItemUseTrigger;

const BRUSH: i32 = 15001;
const USE_BUTTON: u8 = 2;
const FRAME: Duration = Duration::from_millis(50);
const EYE: [f32; 3] = [4.5, 2.620_01, 8.32];
/// The floor cell the fixture's camera ray lands on.
const FLOOR: [i32; 3] = [4, 0, 7];

/// The camera rotation whose ray lands on [`FLOOR`].
fn floor_aim() -> Quat {
    Quat::from_rotation_x(-0.9601)
}

/// The stone floor fixture holding one brush, looking down at the floor. Server
/// components, when given, register the brush as a data-driven item.
fn brush_fixture(
    components: Option<protocol::ItemComponents>,
) -> (World, client_session::CapturedPackets) {
    let (mut world, captured) = floor_fixture(EYE, floor_aim());
    let data_driven = components.is_some();
    let registry = protocol::ItemRegistryEvent {
        entries: [protocol::ItemRegistryEntry {
            identifier: assets::facts::BRUSH_IDENTIFIER.into(),
            network_id: BRUSH,
            component_based: data_driven,
            version: if data_driven {
                protocol::ItemRegistryVersion::DataDriven
            } else {
                protocol::ItemRegistryVersion::Legacy
            },
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(1),
            canonical_empty_component_data: !data_driven,
            item_tags: [].into(),
            components: components.map(std::sync::Arc::new),
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
        stack.network_id = BRUSH;
        stack.block_runtime_id = 0;
        stack.count = 1;
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
    world.init_resource::<client_presentation::aim_assist::AimAssistFrame>();
    world.init_resource::<client_presentation::camera::ServerCameraView>();
    (world, captured)
}

/// What the server received during one frame.
#[derive(Debug, Default, PartialEq)]
struct Sent {
    swings: usize,
    uses: Vec<(Option<ItemUseTrigger>, [i32; 3], bool)>,
    releases: usize,
    /// Whether the frame's simulated tick reports that an item use started.
    started_using: bool,
}

/// Runs one 50 ms frame with Use held or released in the client's frame order: block use,
/// the item-use reconciliation before the tick, the tick, then item use.
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
    world
        .run_system_cached(crate::item_use::adopt_server_item_use)
        .unwrap();
    world
        .run_system_cached(crate::movement::advance_local_physics)
        .unwrap();
    let started_using = world
        .resource::<MovementTicker>()
        .pending_snapshots()
        .iter()
        .any(|sample| {
            sample.flags.bits() & protocol::PlayerInputFlags::START_USING_ITEM.bits() != 0
        });
    produce_air_use(world);
    let packets = captured.drain();
    let transactions = packets
        .iter()
        .filter_map(protocol::test_support::transactions::observe_transaction)
        .collect::<Vec<_>>();
    Sent {
        swings: packets
            .iter()
            .filter(|packet| format!("{:?}", packet.kind()) == "AnimatePacket")
            .count(),
        uses: transactions
            .iter()
            .filter_map(|transaction| transaction.item_use())
            .map(|tx| (tx.trigger, tx.position, tx.predicted_success))
            .collect(),
        releases: transactions
            .iter()
            .filter(|transaction| transaction.release().is_some())
            .count(),
        started_using,
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

fn using(world: &World) -> bool {
    world
        .resource::<client_interaction::item_use::ItemUseRuntime>()
        .is_using()
}

/// Vanilla's brush succeeds on any block: the press predicts success without a swing and
/// starts using the brush, the held button repeats the use on the block, and button-up
/// sends the release that ends the use.
#[test]
fn holding_use_with_a_brush_brushes_until_the_button_comes_up() {
    let (mut world, mut captured) = brush_fixture(None);
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    settle(&mut world, &mut router);

    let press = step(&mut world, &mut router, &mut captured, true);
    assert_eq!(
        press,
        Sent {
            swings: 0,
            uses: vec![(Some(ItemUseTrigger::PlayerInput), FLOOR, true)],
            releases: 0,
            started_using: true,
        }
    );
    assert!(
        using(&world),
        "the brush's use on the block starts using it"
    );

    let mut repeats = Vec::new();
    for _ in 0..10 {
        let sent = step(&mut world, &mut router, &mut captured, true);
        assert_eq!(
            (sent.swings, sent.releases, sent.started_using),
            (0, 0, false),
            "{sent:?}"
        );
        repeats.extend(sent.uses);
        assert!(using(&world));
    }
    assert!(repeats.len() >= 2, "held use repeats: {repeats:?}");
    assert!(
        repeats
            .iter()
            .all(|repeat| *repeat == (Some(ItemUseTrigger::SimulationTick), FLOOR, true)),
        "{repeats:?}"
    );

    let release = step(&mut world, &mut router, &mut captured, false);
    assert_eq!(release.releases, 1, "{release:?}");
    assert!(!using(&world));
}

/// A brush that reaches a block while Use is already held starts its use on that build's
/// tick, before physics simulates it, so that tick's input reports the start.
#[test]
fn a_held_brush_reaching_a_block_starts_its_use_on_the_build_tick() {
    let (mut world, mut captured) = brush_fixture(None);
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    settle(&mut world, &mut router);
    let eye = Vec3::from_array(EYE);
    let sky = frame_origin(&world, eye, Quat::IDENTITY);
    let floor = frame_origin(&world, eye, floor_aim());

    retain_pick(&mut world, &sky);
    let press = step(&mut world, &mut router, &mut captured, true);
    assert!(!press.started_using, "{press:?}");
    assert!(!using(&world), "a brush has no use in the air");

    retain_pick(&mut world, &floor);
    let mut frames = Vec::new();
    while frames.len() < 10 {
        let sent = step(&mut world, &mut router, &mut captured, true);
        let reached = !sent.uses.is_empty();
        frames.push(sent);
        if reached {
            break;
        }
        assert!(!using(&world), "{frames:?}");
    }
    let reached = frames.last().unwrap();
    assert_eq!(
        *reached,
        Sent {
            swings: 0,
            uses: vec![(Some(ItemUseTrigger::SimulationTick), FLOOR, true)],
            releases: 0,
            started_using: true,
        },
        "{frames:?}"
    );
    assert!(using(&world));
}

/// Vanilla's `GameMode::useItemOn` starts the held item's cooldown after any successful use
/// on a block, so a brush's use on a block starts its category timer.
#[test]
fn a_brush_use_on_a_block_starts_its_item_category_cooldown() {
    let (mut world, mut captured) = brush_fixture(Some(protocol::ItemComponents {
        use_cooldown: Some(protocol::ItemUseCooldown {
            category: "fixture:brush".into(),
            ticks: 9,
        }),
        ..Default::default()
    }));
    let cooldown = world
        .resource::<crate::runtime::world::ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .item_use_profile(BRUSH)
        .cooldown
        .unwrap();
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    settle(&mut world, &mut router);
    let tick = world
        .resource::<MovementTicker>()
        .build_action_state()
        .unwrap()
        .tick
        .saturating_add(1);

    let press = step(&mut world, &mut router, &mut captured, true);
    assert_eq!(
        press.uses,
        vec![(Some(ItemUseTrigger::PlayerInput), FLOOR, true)]
    );
    let runtime = world.resource::<client_interaction::item_use::ItemUseRuntime>();
    assert!(!runtime.cooldown_ready(cooldown, tick + 8));
    assert!(runtime.cooldown_ready(cooldown, tick + 9));
}
