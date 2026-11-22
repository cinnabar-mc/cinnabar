//! An item in use ignores block use, as vanilla does.
use super::*;

/// Number of item-use transactions on the fixture chest from one block-use frame, with Use
/// freshly pressed or held, while eating a golden apple or holding it idle.
fn chest_uses(eating: bool, pressed: bool) -> usize {
    let (mut world, mut captured) = fixture();
    if !pressed {
        hold_use(&mut world);
    }
    // An active physics clock lets this frame simulate the tick a repeat precedes.
    let position = world
        .resource::<MovementTicker>()
        .newest_unsent_sample()
        .unwrap()
        .position;
    let mut physics = client_interaction::movement::LocalPhysicsController::default();
    physics.reanchor_network_position(position, 101, true);
    world.insert_resource(physics);
    if eating {
        super::attack_while_using_tests::start_eating(&mut world);
    } else {
        item_use_tests::hold_air_item(&mut world, "minecraft:golden_apple", 2, [].into());
    }
    world.run_system_cached(produce_block_use).unwrap();
    if eating {
        let runtime = world.resource::<BlockUseRuntime>();
        assert!(!runtime.press_pending(), "the ignored press stayed latched");
        // Vanilla keeps repeating the build action while Use is held, so the item's use resumes
        // from the hold once the eat ends.
        assert!(
            !runtime.press_consumed(),
            "the ignored press disabled the held item use"
        );
    }
    captured
        .drain()
        .iter()
        .filter_map(item_use)
        .filter(|tx| tx.position == [4, 2, 6])
        .count()
}

#[test]
fn eating_ignores_the_chest_in_front_of_the_player() {
    for pressed in [false, true] {
        assert_eq!(
            chest_uses(false, pressed),
            1,
            "idle food (pressed {pressed})"
        );
        assert_eq!(
            chest_uses(true, pressed),
            0,
            "eating used the chest (pressed {pressed})"
        );
    }
}

/// A Use press that shares its frame with an Attack tap while eating is dropped at once rather
/// than kept for after the attack, so it cannot use the chest once the eat ends.
#[test]
fn eating_drops_a_use_press_that_shares_a_frame_with_an_attack_tap() {
    let (mut world, _captured) = fixture();
    super::attack_while_using_tests::start_eating(&mut world);
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    let mut snapshot = hold(&mut world, &mut router, Vec::new(), vec![1, 2]);
    // Attack went down and back up within the frame, so it is pressed but no longer held.
    snapshot.phases[Action::Attack as usize].held = false;
    assert!(snapshot.phases[Action::Use as usize].pressed);
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
    world.run_system_cached(produce_block_use).unwrap();
    assert!(
        !world.resource::<BlockUseRuntime>().press_pending(),
        "the ignored press stayed latched"
    );
}

/// A Use tap while eating is not kept by a chest-use stop that a full send queue refuses, so it
/// cannot replay against the chest once the eat stops vetoing and the queue drains.
#[test]
fn a_refused_stop_does_not_keep_a_use_tap_made_while_eating() {
    let (mut world, mut captured) = fixture_with_network(
        NetworkHandle::<PackApplication>::stub_capturing_packets_with_capacity(8),
    );
    world.run_system_cached(produce_block_use).unwrap();
    assert!(
        world
            .resource::<BlockUseRuntime>()
            .last_success_destination()
            .is_some(),
        "the fixture press uses the chest"
    );
    captured.drain();
    super::attack_while_using_tests::start_eating(&mut world);
    set_hotbar_stack(&mut world, 1, None);
    let network = world.resource::<NetworkHandle<PackApplication>>();
    for _ in 0..8 {
        network
            .send_inventory_packet(protocol::swing_arm_packet(42, protocol::SwingSource::Build))
            .unwrap();
    }
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    // Releasing Use stops the chest use; the full queue refuses the stop.
    hold(&mut world, &mut router, Vec::new(), Vec::new());
    world.run_system_cached(produce_block_use).unwrap();
    assert!(world.resource::<BlockUseRuntime>().stopping());
    // A quick tap while eating retries the refused stop.
    let snapshot = hold(&mut world, &mut router, Vec::new(), vec![2]);
    assert!(snapshot.phases[Action::Use as usize].pressed);
    world.run_system_cached(produce_block_use).unwrap();
    hold(&mut world, &mut router, Vec::new(), Vec::new());
    // Selecting another slot ends the veto, then the queue drains and admits the stop.
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .inventory
        .set_local_selected_slot(1);
    captured.drain();
    world.run_system_cached(produce_block_use).unwrap();
    world.run_system_cached(produce_block_use).unwrap();
    let packets = captured.drain();
    let stop = protocol::stop_item_use_on_packet(42, [4, 2, 6]).kind();
    assert!(
        packets.iter().any(|packet| packet.kind() == stop),
        "the stop drains"
    );
    assert!(
        packets
            .iter()
            .filter_map(item_use)
            .all(|tx| tx.trigger != Some(ItemUseTrigger::PlayerInput)),
        "the tap made while eating replayed against the chest"
    );
}
