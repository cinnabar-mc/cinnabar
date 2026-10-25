//! Placement and air use share admitted item cooldown categories.
use super::*;

/// Holds a component-based stone placer with an explicit shared category cooldown.
fn fixture() -> (
    World,
    client_session::CapturedPackets,
    client_world::ResolvedUseCooldown,
) {
    let (mut world, captured) =
        floor_fixture([4.5, 2.620_01, 8.32], Quat::from_rotation_x(-0.9601));
    let entry = protocol::ItemRegistryEntry {
        identifier: "fixture:cooled_placer".into(),
        network_id: 15000,
        component_based: true,
        version: protocol::ItemRegistryVersion::DataDriven,
        component_digest: [0; 32],
        negotiated_max_stack_size: Some(64),
        canonical_empty_component_data: false,
        item_tags: [].into(),
        components: Some(std::sync::Arc::new(protocol::ItemComponents {
            food: true,
            food_can_always_eat: Some(true),
            use_duration_ticks: Some(20),
            use_animation: Some("eat".into()),
            use_cooldown: Some(protocol::ItemUseCooldown {
                category: "fixture:shared".into(),
                ticks: 9,
            }),
            ..Default::default()
        })),
    };
    let registry = protocol::ItemRegistryEvent {
        entries: [entry].into(),
    };
    let cooldown = {
        let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        assert!(stream.seed_item_registry(registry.clone()));
        stream.authority().item_use_profile(15000).cooldown.unwrap()
    };
    {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        let mut stack = player.selected_stack().unwrap().clone();
        player.inventory.ledger_mut().apply_registry(&registry);
        stack.network_id = 15000;
        player
            .inventory
            .ledger_mut()
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
    }
    (world, captured, cooldown)
}

/// A successful placement starts the same category timer that air use reads.
#[test]
fn an_admitted_placement_starts_its_item_category_cooldown() {
    let (mut world, mut captured, cooldown) = fixture();
    let tick = world
        .resource::<MovementTicker>()
        .build_action_state()
        .unwrap()
        .tick
        .saturating_add(1);
    world.run_system_cached(produce_block_use).unwrap();
    assert!(
        captured
            .drain()
            .iter()
            .any(|packet| super::frame_picks::successful_placement(packet).is_some())
    );
    let runtime = world.resource::<client_interaction::item_use::ItemUseRuntime>();
    assert!(!runtime.cooldown_ready(cooldown, tick + 8));
    assert!(runtime.cooldown_ready(cooldown, tick + 9));
}

/// Even a fresh placement press cannot consume a block while its category is cooling down.
#[test]
fn a_category_cooldown_blocks_placement_without_blocking_the_transaction() {
    let (mut world, mut captured, cooldown) = fixture();
    let tick = world
        .resource::<MovementTicker>()
        .build_action_state()
        .unwrap()
        .tick
        .saturating_add(1);
    let mut runtime = world.resource_mut::<client_interaction::item_use::ItemUseRuntime>();
    runtime.synchronize(7);
    runtime.start_cooldown(cooldown, tick);
    world.run_system_cached(produce_block_use).unwrap();
    let uses = captured
        .drain()
        .iter()
        .filter_map(item_use)
        .collect::<Vec<_>>();
    assert_eq!(uses.len(), 1);
    assert!(!uses[0].predicted_success);
    assert!(
        world
            .resource::<BlockUseRuntime>()
            .last_success_destination()
            .is_none()
    );
}

/// A refused placement cannot start an item cooldown before its transaction is admitted.
#[test]
fn a_refused_placement_leaves_its_item_category_ready() {
    let (mut world, _, cooldown) = fixture();
    let tick = world
        .resource::<MovementTicker>()
        .build_action_state()
        .unwrap()
        .tick
        .saturating_add(1);
    let (network, _guard) = NetworkHandle::<PackApplication>::with_command_capacity(1);
    network
        .send_inventory_packet(protocol::swing_arm_packet(42, protocol::SwingSource::Build))
        .unwrap();
    world.insert_resource(network);
    world.run_system_cached(produce_block_use).unwrap();
    assert!(
        world
            .resource::<client_interaction::item_use::ItemUseRuntime>()
            .cooldown_ready(cooldown, tick)
    );
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    world.insert_resource(network);
    hold_use(&mut world);
    world.run_system_cached(produce_block_use).unwrap();
    assert!(
        captured
            .drain()
            .iter()
            .any(|packet| super::frame_picks::successful_placement(packet).is_some())
    );
    assert!(
        !world
            .resource::<client_interaction::item_use::ItemUseRuntime>()
            .cooldown_ready(cooldown, tick)
    );
}

/// Consuming the final block clears the native item before the category-start callback runs.
#[test]
fn placing_the_final_item_does_not_start_an_empty_stack_cooldown() {
    let (mut world, mut captured, cooldown) = fixture();
    let tick = world
        .resource::<MovementTicker>()
        .build_action_state()
        .unwrap()
        .tick
        .saturating_add(1);
    {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        let mut stack = player.selected_stack().unwrap().clone();
        stack.count = 1;
        player
            .inventory
            .ledger_mut()
            .apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity::window(0),
                    slot: 0,
                },
                stack,
                storage_item: None,
            }));
    }
    world.run_system_cached(produce_block_use).unwrap();
    assert!(
        captured
            .drain()
            .iter()
            .any(|packet| super::frame_picks::successful_placement(packet).is_some())
    );
    assert!(
        world
            .resource::<client_state::PlayerRuntime>()
            .selected_stack()
            .is_none()
    );
    assert!(
        world
            .resource::<client_interaction::item_use::ItemUseRuntime>()
            .cooldown_ready(cooldown, tick)
    );
}

/// Air-use maintenance must not retire a placement timer one completed tick before its deadline.
#[test]
fn air_use_maintenance_cannot_expire_a_placement_cooldown_early() {
    let (mut world, mut captured, cooldown) = fixture();
    world.init_resource::<client_presentation::aim_assist::AimAssistFrame>();
    world.init_resource::<client_presentation::camera::ServerCameraView>();
    super::frame_picks::warm_physics(&mut world);
    world.run_system_cached(produce_block_use).unwrap();
    assert!(
        captured
            .drain()
            .iter()
            .any(|packet| super::frame_picks::successful_placement(packet).is_some())
    );
    hold_use(&mut world);
    for _ in 0..cooldown.ticks - 1 {
        world
            .resource_mut::<Time<Real>>()
            .advance_by(simulation_clock::TICK_DURATION);
        world
            .run_system_cached(crate::movement::advance_local_physics)
            .unwrap();
        produce_air_use(&mut world);
        captured.drain();
    }
    let tick = world
        .resource::<MovementTicker>()
        .build_action_state()
        .unwrap()
        .tick;
    assert!(
        !world
            .resource::<client_interaction::item_use::ItemUseRuntime>()
            .cooldown_ready(cooldown, tick + 1)
    );
    world
        .resource_mut::<Time<Real>>()
        .advance_by(simulation_clock::TICK_DURATION);
    world
        .run_system_cached(crate::movement::advance_local_physics)
        .unwrap();
    produce_air_use(&mut world);
    let tick = world
        .resource::<MovementTicker>()
        .build_action_state()
        .unwrap()
        .tick;
    assert!(
        world
            .resource::<client_interaction::item_use::ItemUseRuntime>()
            .cooldown_ready(cooldown, tick + 1)
    );
}
