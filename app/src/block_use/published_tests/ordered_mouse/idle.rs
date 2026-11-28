//! Frames without input edges or pending carriers skip structural and dispatch work.

use super::*;
use crate::ordered_interactions::InteractionRuntime;

#[test]
fn idle_ordered_frames_keep_the_runtime_in_place_and_still_admit_later_taps() {
    let (mut world, mut captured) = mouse_fixture();
    mouse_frame(
        &mut world,
        MouseButton::Left,
        &[ButtonState::Pressed, ButtonState::Released],
    );
    assert_eq!(attack_count(&mut captured), 1);
    let added = world
        .get_resource_change_ticks::<InteractionRuntime>()
        .expect("ordered input keeps its runtime")
        .added;
    for _ in 0..3 {
        // Each frame drains before movement twice and after movement once.
        mouse_frame(&mut world, MouseButton::Left, &[]);
        assert_eq!(attack_count(&mut captured), 0);
        assert_eq!(
            world
                .get_resource_change_ticks::<InteractionRuntime>()
                .map(|ticks| ticks.added),
            Some(added),
            "an idle drain must not remove and reinsert the runtime"
        );
    }
    mouse_frame(
        &mut world,
        MouseButton::Left,
        &[ButtonState::Pressed, ButtonState::Released],
    );
    assert_eq!(
        attack_count(&mut captured),
        1,
        "a later tap is still admitted"
    );
}

#[test]
fn block_stop_fence_admits_without_its_context_when_no_stop_is_pending() {
    let mut world = World::new();
    world.init_resource::<BlockUseRuntime>();
    // The fence system's block-use context is absent, so admission must not run it. Pending
    // stops still run it: `a_refused_stop_does_not_keep_a_use_tap_made_while_eating` drains one.
    assert!(crate::block_use::ordered::run_fence(&mut world, false));
    assert!(crate::block_use::ordered::run_fence(&mut world, true));
}
