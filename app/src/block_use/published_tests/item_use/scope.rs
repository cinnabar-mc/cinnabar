//! A new world's server use survives the first ordered input hand-off.

use super::*;
use client_interaction::{item_use::ItemUseRuntime, movement::LocalPhysicsController};

/// Delivers a new-world server start and a Use release while item use still owns the old scope.
fn server_start_after_scope_change(previous_session: u64, previous_dimension: i32) {
    let (mut world, mut captured) = fixture();
    hold_air_item(&mut world, "minecraft:bow", 1, [].into());
    world.init_resource::<crate::melee::MeleeTarget>();
    {
        let mut runtime = world.resource_mut::<ItemUseRuntime>();
        runtime.synchronize(previous_session);
        runtime.observe_world(SessionGeneration::new(previous_session), previous_dimension);
    }
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([4.5, 2.620_01, 8.5], 101, false);
    physics.apply_server_movement_flags(
        0,
        client_world::MovementFlagUpdate {
            using_item: Some(true),
            ..Default::default()
        },
    );
    world.insert_resource(physics);
    world
        .resource_mut::<Time<Real>>()
        .advance_by(simulation_clock::TICK_DURATION);
    let mut events = semantic_input::DesktopEvents::default();
    events.push(semantic_input::DesktopEvent::Button {
        control: semantic_input::PhysicalControl::MouseButton(2),
        pressed: false,
        modifiers: Default::default(),
    });
    let snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            desktop_events: Some(events),
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame::default()),
            ..Default::default()
        })
        .unwrap();
    assert!(snapshot.phases[Action::Use as usize].released);
    assert_eq!(snapshot.ordered.as_ref().unwrap().events().len(), 1);
    world.insert_resource(SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
    world.run_system_cached(produce_block_use).unwrap();
    world
        .run_system_cached(crate::item_use::adopt_server_item_use)
        .unwrap();
    assert!(
        world.resource::<ItemUseRuntime>().is_using(),
        "the new world's server start must survive the ordered Use edge"
    );
    assert_eq!(
        world
            .resource::<ItemUseRuntime>()
            .active_timing()
            .unwrap()
            .0,
        world.resource::<MovementTicker>().next_tick()
    );
    assert_eq!(
        world
            .resource_mut::<LocalPhysicsController>()
            .take_server_using_item(),
        None
    );
    assert!(
        captured.drain().iter().all(|packet| {
            protocol::test_support::transactions::observe_transaction(packet).is_none()
        }),
        "the server start needs no local use or release transaction"
    );
}

#[test]
fn server_item_use_survives_an_ordered_edge_after_a_session_change() {
    server_start_after_scope_change(6, 0);
}

#[test]
fn server_item_use_survives_an_ordered_edge_after_a_dimension_change() {
    server_start_after_scope_change(7, 1);
}

/// Supplies two verified bow slots, an acknowledged movement sample and an air pick.
fn bow_world(held: bool) -> (World, client_session::CapturedPackets) {
    let (mut world, mut captured) = fixture();
    hold_air_item(&mut world, "minecraft:bow", 1, [].into());
    world.init_resource::<crate::melee::MeleeTarget>();
    world.init_resource::<client_state::GameplayScreenControls>();
    world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        ui.publish_local_runtime_id(&mut player, 7, 42).unwrap();
        let ledger = ui.inventory_ledger_mut(&mut player);
        let mut stack = ledger.backing_stack(0).unwrap().clone();
        stack.stack_network_id += 1;
        ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity {
                    window_id: Some(0),
                    slot_type: None,
                    dynamic_id: None,
                },
                slot: 1,
            },
            stack,
            storage_item: None,
        }));
    });
    world
        .resource_mut::<InteractionOriginSnapshot>()
        .assist_direction(Vec3::Y);
    world.resource_mut::<BlockUseRuntime>().clear_picks();
    world
        .run_system_cached(crate::block_use::retain_block_use_pick)
        .unwrap();
    super::super::riptide_tests::flush_and_acknowledge(&mut world);
    world
        .run_system_cached(crate::item_use::ordered::synchronize_scope)
        .unwrap();
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([4.5, 2.620_01, 8.5], 101, false);
    world.insert_resource(physics);
    world
        .resource_mut::<Time<Real>>()
        .advance_by(simulation_clock::TICK_DURATION);
    let mut events = semantic_input::DesktopEvents::default();
    if !held {
        events.push(semantic_input::DesktopEvent::Button {
            control: semantic_input::PhysicalControl::MouseButton(2),
            pressed: false,
            modifiers: Default::default(),
        });
    }
    route_use_events(&mut world, events, if held { vec![2] } else { Vec::new() });
    captured.drain();
    (world, captured)
}

/// Starts a server-owned use without introducing a new local Use press.
fn server_using_bow(held: bool) -> (World, client_session::CapturedPackets) {
    let (mut world, mut captured) = bow_world(held);
    world
        .resource_mut::<LocalPhysicsController>()
        .apply_server_movement_flags(
            0,
            client_world::MovementFlagUpdate {
                using_item: Some(true),
                ..Default::default()
            },
        );
    world
        .run_system_cached(crate::item_use::adopt_server_item_use)
        .unwrap();
    assert!(world.resource::<ItemUseRuntime>().is_using());
    captured.drain();
    (world, captured)
}

/// Finalizes one ordered batch against the fixture's current button state.
fn route_use_events(world: &mut World, events: semantic_input::DesktopEvents, buttons: Vec<u8>) {
    let snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            desktop_events: Some(events),
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                mouse_buttons: buttons,
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    world.insert_resource(SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
}

/// A slot change clears an active use, while an earlier game-mode stop preserves a later start.
#[test]
fn a_local_reselection_orders_game_mode_stops_and_pending_server_starts() {
    for game_mode_stop in [false, true] {
        let (mut world, mut captured) = server_using_bow(true);
        {
            let mut physics = world.resource_mut::<LocalPhysicsController>();
            if game_mode_stop {
                physics.stop_item_use_for_game_mode();
            }
            physics.apply_server_movement_flags(
                0,
                client_world::MovementFlagUpdate {
                    using_item: Some(true),
                    ..Default::default()
                },
            );
        }
        let mut events = semantic_input::DesktopEvents::default();
        events.push(semantic_input::DesktopEvent::Scroll { next: true });
        events.push(semantic_input::DesktopEvent::Button {
            control: semantic_input::PhysicalControl::MouseButton(2),
            pressed: false,
            modifiers: Default::default(),
        });
        route_use_events(&mut world, events, Vec::new());
        world.run_system_cached(produce_block_use).unwrap();
        assert_eq!(
            world
                .resource::<client_state::PlayerRuntime>()
                .selected_hotbar_slot(),
            Some(1)
        );
        world
            .run_system_cached(crate::item_use::adopt_server_item_use)
            .unwrap();
        let runtime = world.resource::<ItemUseRuntime>();
        assert_eq!(
            runtime.is_using(),
            game_mode_stop,
            "the latest item-use transition wins"
        );
        assert_eq!(runtime.movement_modifier().is_some(), game_mode_stop);
        assert!(!runtime.has_server_using_item());
        assert_eq!(
            world.resource::<LocalPhysicsController>().live_using_item(),
            game_mode_stop
        );
        assert!(captured.drain().iter().all(|packet| {
            protocol::test_support::transactions::observe_transaction(packet).is_none()
        }));
    }
}

/// Deferred button transitions preserve a real release without inventing one for a held press.
#[test]
fn deferred_use_edges_keep_server_clears_in_input_order() {
    for buttons in [vec![true], vec![true, false]] {
        let releasing = buttons == [true, false];
        let (mut world, mut captured) = server_using_bow(false);
        world
            .resource_mut::<LocalPhysicsController>()
            .apply_server_movement_flags(
                0,
                client_world::MovementFlagUpdate {
                    using_item: Some(false),
                    ..Default::default()
                },
            );
        let mut events = semantic_input::DesktopEvents::default();
        for (button, pressed) in [(3, true), (3, false)] {
            events.push(semantic_input::DesktopEvent::Button {
                control: semantic_input::PhysicalControl::MouseButton(button),
                pressed,
                modifiers: Default::default(),
            });
        }
        for pressed in &buttons {
            events.push(semantic_input::DesktopEvent::Button {
                control: semantic_input::PhysicalControl::MouseButton(2),
                pressed: *pressed,
                modifiers: Default::default(),
            });
        }
        let held = if *buttons.last().unwrap() {
            vec![2]
        } else {
            Vec::new()
        };
        route_use_events(&mut world, events, held);
        world.run_system_cached(produce_block_use).unwrap();
        world
            .run_system_cached(crate::item_use::adopt_server_item_use)
            .unwrap();
        assert_eq!(
            world.resource::<ItemUseRuntime>().is_using(),
            releasing,
            "only a real owned release may precede the server clear: {buttons:?}"
        );
        let mut windows = crate::ordered_interactions::PrimaryWindows::new(&mut world);
        crate::ordered_interactions::drain(
            &mut world,
            crate::ordered_interactions::DispatchPhase::AfterMovement,
            &mut windows,
        );
        assert!(!world.resource::<ItemUseRuntime>().is_using());
        let transactions: Vec<_> = captured
            .drain()
            .iter()
            .filter_map(protocol::test_support::transactions::observe_transaction)
            .collect();
        assert_eq!(
            transactions
                .iter()
                .filter(|transaction| transaction.release().is_some())
                .count(),
            usize::from(releasing)
        );
        assert_eq!(
            transactions
                .iter()
                .filter(|transaction| transaction.item_use().is_some())
                .count(),
            usize::from(!releasing),
            "a fresh press may attempt use only after the server cleared the old use: {buttons:?}",
        );
    }
}

/// Expiring a press/release/re-press behind a full outbox still applies the server stop.
#[test]
fn a_server_clear_survives_expired_use_edges_and_backpressure() {
    let (mut world, _) = server_using_bow(false);
    let (network, mut captured) =
        NetworkHandle::<PackApplication>::stub_capturing_packets_with_capacity(1);
    network
        .send_inventory_packets(vec![protocol::swing_arm_packet(
            42,
            protocol::SwingSource::UseItem,
        )])
        .unwrap();
    world.insert_resource(network);
    {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        let mode = player.facts.player_game_mode();
        player.inventory.queue_local_hotbar_selection(1, mode);
        player.inventory.queue_local_hotbar_selection(0, mode);
    }
    world
        .resource_mut::<LocalPhysicsController>()
        .apply_server_movement_flags(
            0,
            client_world::MovementFlagUpdate {
                using_item: Some(false),
                ..Default::default()
            },
        );
    let mut events = semantic_input::DesktopEvents::default();
    for pressed in [true, false, true] {
        events.push(semantic_input::DesktopEvent::Button {
            control: semantic_input::PhysicalControl::MouseButton(2),
            pressed,
            modifiers: Default::default(),
        });
    }
    route_use_events(&mut world, events, vec![2]);
    world.run_system_cached(produce_block_use).unwrap();
    world
        .run_system_cached(crate::item_use::adopt_server_item_use)
        .unwrap();
    assert!(world.resource::<ItemUseRuntime>().is_using());
    let mut windows = crate::ordered_interactions::PrimaryWindows::new(&mut world);
    crate::ordered_interactions::drain(
        &mut world,
        crate::ordered_interactions::DispatchPhase::AfterMovement,
        &mut windows,
    );
    captured.drain();
    world
        .run_system_cached(crate::item_use::produce_item_use)
        .unwrap();
    assert!(
        !world.resource::<ItemUseRuntime>().is_using(),
        "expired input must not discard the server stop"
    );
    assert_eq!(world.resource::<ItemUseRuntime>().movement_modifier(), None);
    assert!(!world.resource::<LocalPhysicsController>().live_using_item());
    assert!(captured.drain().iter().all(|packet| {
        protocol::test_support::transactions::observe_transaction(packet).is_none()
    }));
}

/// A real unordered button-up reaches maintenance before a same-frame server stop.
#[test]
fn an_unordered_release_precedes_a_same_frame_server_clear() {
    let (mut world, mut captured) = server_using_bow(true);
    world
        .resource_mut::<LocalPhysicsController>()
        .apply_server_movement_flags(
            0,
            client_world::MovementFlagUpdate {
                using_item: Some(false),
                ..Default::default()
            },
        );
    let snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame::default()),
            ..Default::default()
        })
        .unwrap();
    assert!(snapshot.ordered.is_none());
    assert!(snapshot.phases[Action::Use as usize].released);
    world.insert_resource(SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
    world.run_system_cached(produce_block_use).unwrap();
    world
        .run_system_cached(crate::item_use::adopt_server_item_use)
        .unwrap();
    world
        .run_system_cached(crate::item_use::produce_item_use)
        .unwrap();
    assert!(!world.resource::<ItemUseRuntime>().is_using());
    assert_eq!(
        captured
            .drain()
            .iter()
            .filter_map(protocol::test_support::transactions::observe_transaction)
            .filter(|transaction| transaction.release().is_some())
            .count(),
        1
    );
}

/// Unordered adapter taps take and release ownership of an already active server use.
#[test]
fn a_complete_unordered_tap_releases_a_server_started_use() {
    let (mut world, mut captured) = server_using_bow(false);
    let snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                mouse_edges: semantic_input::ButtonEdges {
                    pressed: vec![2],
                    released: vec![2],
                },
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    assert!(snapshot.ordered.is_none());
    let phase = snapshot.phases[Action::Use as usize];
    assert!(phase.pressed && phase.released && !phase.held);
    world.insert_resource(SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
    world.run_system_cached(produce_block_use).unwrap();
    world
        .run_system_cached(crate::item_use::adopt_server_item_use)
        .unwrap();
    world
        .run_system_cached(crate::item_use::produce_item_use)
        .unwrap();
    assert!(!world.resource::<ItemUseRuntime>().is_using());
    assert_eq!(world.resource::<ItemUseRuntime>().movement_modifier(), None);
    let transactions: Vec<_> = captured
        .drain()
        .iter()
        .filter_map(protocol::test_support::transactions::observe_transaction)
        .collect();
    assert_eq!(
        transactions
            .iter()
            .filter(|transaction| transaction.release().is_some())
            .count(),
        1
    );
    assert!(
        transactions
            .iter()
            .all(|transaction| transaction.item_use().is_none())
    );
}

/// A later server stop retires local writes from the old use before its following start.
#[test]
fn a_server_control_retires_older_local_item_use_writes() {
    let (mut world, mut captured) = server_using_bow(true);
    world
        .run_system_cached_with(crate::hotbar::ordered::select, Action::Hotbar2)
        .unwrap();
    {
        let mut physics = world.resource_mut::<LocalPhysicsController>();
        physics.stop_item_use_for_game_mode();
        physics.apply_server_movement_flags(
            0,
            client_world::MovementFlagUpdate {
                using_item: Some(true),
                ..Default::default()
            },
        );
    }
    route_use_events(
        &mut world,
        semantic_input::DesktopEvents::default(),
        vec![2],
    );
    world.run_system_cached(produce_block_use).unwrap();
    world
        .run_system_cached(crate::item_use::adopt_server_item_use)
        .unwrap();
    assert!(
        world.resource::<ItemUseRuntime>().is_using(),
        "the later server start must survive old local writes"
    );
    assert!(world.resource::<LocalPhysicsController>().live_using_item());
    assert!(captured.drain().iter().all(|packet| {
        protocol::test_support::transactions::observe_transaction(packet).is_none()
    }));
}

/// A queued local press keeps its click transaction instead of becoming a silent server use.
#[test]
fn local_use_presses_precede_pending_server_starts() {
    for ordered in [false, true] {
        let (mut world, mut captured) = bow_world(false);
        world
            .resource_mut::<LocalPhysicsController>()
            .apply_server_movement_flags(
                0,
                client_world::MovementFlagUpdate {
                    using_item: Some(true),
                    ..Default::default()
                },
            );
        if ordered {
            let mut events = semantic_input::DesktopEvents::default();
            for (button, pressed) in [(3, true), (3, false), (2, true)] {
                events.push(semantic_input::DesktopEvent::Button {
                    control: semantic_input::PhysicalControl::MouseButton(button),
                    pressed,
                    modifiers: Default::default(),
                });
            }
            route_use_events(&mut world, events, vec![2]);
        } else {
            let snapshot = world
                .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
                .route_and_finalize(semantic_input::DeviceFrame {
                    keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                        mouse_buttons: vec![2],
                        ..Default::default()
                    }),
                    ..Default::default()
                })
                .unwrap();
            assert!(snapshot.phases[Action::Use as usize].pressed);
            assert!(snapshot.ordered.is_none());
            world.insert_resource(SemanticInputSnapshot(
                semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
            ));
        }
        world.run_system_cached(produce_block_use).unwrap();
        world
            .run_system_cached(crate::item_use::adopt_server_item_use)
            .unwrap();
        assert!(
            !world.resource::<ItemUseRuntime>().is_using(),
            "a local press must reach admission before a server start"
        );
        world
            .run_system_cached(crate::item_use::produce_item_use)
            .unwrap();
        assert_eq!(
            captured
                .drain()
                .iter()
                .filter_map(protocol::test_support::transactions::observe_transaction)
                .filter(|transaction| transaction.item_use().is_some())
                .count(),
            1
        );
    }
}

/// An absorbing modal releases an unowned server use even without a menu or UI-focus flag.
#[test]
fn an_absorbing_screen_releases_a_server_started_use() {
    let (mut world, mut captured) = server_using_bow(false);
    world
        .resource_mut::<client_state::ScreenFocus>()
        .absorbs_input = true;
    route_use_events(
        &mut world,
        semantic_input::DesktopEvents::default(),
        Vec::new(),
    );
    world
        .run_system_cached(crate::item_use::produce_item_use)
        .unwrap();
    assert!(
        !world.resource::<ItemUseRuntime>().is_using(),
        "absorbing screens must release server-started item use"
    );
    assert_eq!(world.resource::<ItemUseRuntime>().movement_modifier(), None);
    assert_eq!(
        captured
            .drain()
            .iter()
            .filter_map(protocol::test_support::transactions::observe_transaction)
            .filter(|transaction| transaction.release().is_some())
            .count(),
        1
    );
}

/// Releasing one Use binding keeps the action held and must not defer a server stop.
#[test]
fn a_remaining_use_binding_does_not_hide_a_server_clear() {
    let (mut world, mut captured) = bow_world(false);
    let defaults = semantic_input::ControlSettings::default();
    let mut bindings = defaults.bindings().to_vec();
    bindings.push(semantic_input::ActionBinding {
        action: Action::Use,
        context: semantic_input::InputContext::Gameplay,
        chord: semantic_input::InputChord {
            control: semantic_input::PhysicalControl::MouseButton(5),
            modifiers: Default::default(),
        },
    });
    world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .replace_bindings(
            semantic_input::ControlSettings::new(
                bindings,
                defaults.mouse_sensitivity,
                defaults.gamepad_look_sensitivity,
                defaults.touch_look_sensitivity,
                defaults.invert_mouse_y,
                defaults.invert_gamepad_y,
                defaults.gamepad_move_deadzone,
                defaults.gamepad_look_deadzone,
            )
            .unwrap(),
        )
        .unwrap();
    // Finish the binding-change barrier, then establish both held controls.
    route_use_events(&mut world, Default::default(), Vec::new());
    route_use_events(&mut world, Default::default(), vec![2, 5]);
    world
        .resource_mut::<LocalPhysicsController>()
        .apply_server_movement_flags(
            0,
            client_world::MovementFlagUpdate {
                using_item: Some(true),
                ..Default::default()
            },
        );
    world
        .run_system_cached(crate::item_use::adopt_server_item_use)
        .unwrap();
    assert!(world.resource::<ItemUseRuntime>().is_using());
    captured.drain();
    world
        .resource_mut::<LocalPhysicsController>()
        .apply_server_movement_flags(
            0,
            client_world::MovementFlagUpdate {
                using_item: Some(false),
                ..Default::default()
            },
        );
    let mut events = semantic_input::DesktopEvents::default();
    for (button, pressed) in [(3, true), (3, false), (2, false)] {
        events.push(semantic_input::DesktopEvent::Button {
            control: semantic_input::PhysicalControl::MouseButton(button),
            pressed,
            modifiers: Default::default(),
        });
    }
    route_use_events(&mut world, events, vec![5]);
    let input = world
        .resource::<SemanticInputSnapshot>()
        .snapshot()
        .unwrap();
    let release = input.ordered.as_ref().unwrap().events().last().unwrap();
    assert_eq!(release.action, Action::Use);
    assert!(!release.pressed && release.buttons.held(Action::Use));
    world.run_system_cached(produce_block_use).unwrap();
    world
        .run_system_cached(crate::item_use::adopt_server_item_use)
        .unwrap();
    assert!(
        !world.resource::<ItemUseRuntime>().is_using(),
        "a held alternate binding must not invent a release ahead of the server clear"
    );
    assert_eq!(world.resource::<ItemUseRuntime>().movement_modifier(), None);
    assert!(!world.resource::<LocalPhysicsController>().live_using_item());
    let mut windows = crate::ordered_interactions::PrimaryWindows::new(&mut world);
    crate::ordered_interactions::drain(
        &mut world,
        crate::ordered_interactions::DispatchPhase::AfterMovement,
        &mut windows,
    );
    assert!(captured.drain().iter().all(|packet| {
        protocol::test_support::transactions::observe_transaction(packet).is_none()
    }));
}
