//! Surfacing stops sprinting until a held sprint control restarts it.

use super::{
    HeldInput, ModeIntent, MovementInput, MovementMode, PhysicsSampleContext, PlayerInputFlags,
    PoolFloor, TICK, grounded_controller, has, input_flags, step,
};

/// The tick whose dry swimming body ends the swim also ends the sprint, so the
/// wire carries both stops. A latched request alone cannot restart it; a held
/// sprint control does.
#[test]
fn a_swim_that_leaves_the_water_stops_and_then_restarts_the_sprint() {
    let world = PoolFloor(std::cell::Cell::new(true));
    let mut physics = grounded_controller();
    let swim = MovementInput {
        forward: 1.0,
        sprinting: true,
        ..MovementInput::default()
    };
    let mut previous = HeldInput::default();
    for _ in 0..12 {
        let sample = step(&mut physics, swim, ModeIntent::default(), &world);
        assert_eq!(sample.processed.mode, MovementMode::Swimming);
        assert!(sample.processed.sprinting);
        previous = HeldInput::from(&sample);
    }
    world.0.set(false);
    let surfaced = step(&mut physics, swim, ModeIntent::default(), &world);
    let flags = input_flags(&surfaced, previous);
    assert_eq!(surfaced.processed.mode, MovementMode::Walking);
    assert!(!surfaced.processed.sprinting);
    assert!(has(flags, PlayerInputFlags::STOP_SWIMMING));
    assert!(has(flags, PlayerInputFlags::STOP_SPRINTING));
    let latched = step(&mut physics, swim, ModeIntent::default(), &world);
    assert!(!latched.processed.sprinting);
    previous = HeldInput::from(&latched);
    let held = physics.advance_with_context(
        TICK,
        swim,
        PhysicsSampleContext {
            input: super::super::TickInput {
                sprint_down: true,
                ..Default::default()
            },
            ..PhysicsSampleContext::default()
        },
        &world,
    );
    let next = &held.samples[0];
    assert!(next.processed.sprinting);
    assert!(has(
        input_flags(next, previous),
        PlayerInputFlags::START_SPRINTING
    ));
}

/// Within one frame, a surfacing tick's sprint stop also ends the latched request,
/// so a later catch-up tick restarts only from a held sprint control.
#[test]
fn a_surfacing_sprint_stop_holds_across_catch_up_ticks_without_a_sprint_control() {
    for sprint_down in [false, true] {
        let world = PoolFloor(std::cell::Cell::new(true));
        let mut physics = grounded_controller();
        let swim = MovementInput {
            forward: 1.0,
            sprinting: true,
            ..MovementInput::default()
        };
        for _ in 0..12 {
            step(&mut physics, swim, ModeIntent::default(), &world);
        }
        world.0.set(false);
        let frame = physics.advance_with_context(
            TICK * 2,
            swim,
            PhysicsSampleContext {
                input: super::super::TickInput {
                    sprint_down,
                    ..Default::default()
                },
                ..PhysicsSampleContext::default()
            },
            &world,
        );
        assert!(frame.blocked.is_none(), "blocked: {:?}", frame.blocked);
        let sprinting: Vec<bool> = frame
            .samples
            .iter()
            .map(|sample| sample.processed.sprinting)
            .collect();
        assert_eq!(sprinting, [false, sprint_down], "sprint_down {sprint_down}");
    }
}
