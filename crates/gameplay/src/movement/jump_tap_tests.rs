//! Jump reaches movement only as the fixed tick samples it, which is also what the tick's
//! `Jumping` input tells the server; a press released before the tick runs does nothing.

use semantic_input::ActionPhase;

use super::*;

const UP: ActionPhase = ActionPhase {
    held: false,
    pressed: false,
    released: false,
};
const PRESSED: ActionPhase = ActionPhase {
    held: true,
    pressed: true,
    released: false,
};
const HELD: ActionPhase = ActionPhase {
    held: true,
    pressed: false,
    released: false,
};
const RELEASED: ActionPhase = ActionPhase {
    held: false,
    pressed: false,
    released: true,
};

/// Runs one render frame of `elapsed` with the given jump phase and returns its completed ticks.
fn frame(
    physics: &mut LocalPhysicsController,
    elapsed: Duration,
    jump: ActionPhase,
    intent: ModeIntent,
) -> Vec<PhysicsMovementSample> {
    let frame = physics.advance_with_context(
        elapsed,
        MovementInput {
            jumping: jump.held,
            ..MovementInput::default()
        },
        PhysicsSampleContext {
            input: super::super::TickInput {
                jump,
                ..Default::default()
            },
            mode_intent: intent,
            ..Default::default()
        },
        &VersionedFloor(1),
    );
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
    frame.samples
}

/// Presses jump in a frame with no tick due, then runs the next tick with jump in `then`.
fn press_before_tick(
    physics: &mut LocalPhysicsController,
    then: ActionPhase,
    intent: ModeIntent,
) -> PhysicsMovementSample {
    let early = frame(physics, Duration::from_millis(10), PRESSED, intent);
    assert!(early.is_empty(), "the press frame must not run a tick");
    let mut samples = frame(physics, Duration::from_millis(45), then, intent);
    assert_eq!(samples.len(), 1);
    samples.remove(0)
}

/// A falling player with an elytra ready, a few ticks below its anchor.
fn falling_with_elytra() -> (LocalPhysicsController, ModeIntent) {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 50.620_01, 0.0], 100, false);
    let intent = ModeIntent {
        elytra_ready: true,
        ..ModeIntent::default()
    };
    for _ in 0..3 {
        assert_eq!(frame(&mut physics, TICK, UP, intent).len(), 1);
    }
    (physics, intent)
}

#[test]
fn a_jump_tap_released_before_the_tick_does_not_deploy_the_elytra() {
    let (mut physics, intent) = falling_with_elytra();
    let tap = press_before_tick(&mut physics, RELEASED, intent);
    assert!(!tap.jumping, "the tick sends no Jumping");
    assert_ne!(tap.processed.mode, MovementMode::Gliding);
    assert!(!has(
        input_flags(&tap, HeldInput::default()),
        PlayerInputFlags::START_GLIDING
    ));

    let (mut physics, intent) = falling_with_elytra();
    let held = press_before_tick(&mut physics, HELD, intent);
    assert!(held.jumping);
    assert_eq!(held.processed.mode, MovementMode::Gliding);
}

#[test]
fn a_jump_tap_released_before_the_tick_does_not_jump_from_the_ground() {
    let mut physics = settled_controller();
    let tap = press_before_tick(&mut physics, RELEASED, ModeIntent::default());
    assert!(!tap.jumping, "the tick sends no Jumping");
    assert!(!tap.processed.jump_initiated);
    assert!(tap.grounded_after_tick);

    let mut physics = settled_controller();
    let held = press_before_tick(&mut physics, HELD, ModeIntent::default());
    assert!(held.processed.jump_initiated);
    assert!(!held.grounded_after_tick);
}
