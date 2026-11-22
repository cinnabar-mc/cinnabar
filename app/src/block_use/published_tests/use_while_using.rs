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
    if eating && pressed {
        assert!(
            world.resource::<BlockUseRuntime>().press_consumed(),
            "the ignored press was left for item use"
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
