use super::*;

/// One local player driven through whole gameplay frames.
struct Player {
    physics: LocalPhysicsController,
    ticker: MovementTicker,
    locals: super::super::LocomotionState,
    effects: super::super::LocalMovementEffectTimeline,
    speed: super::super::LocalMovementSpeedAuthority,
}

impl Player {
    fn new() -> Self {
        let (physics, ticker) = walked_physics(0);
        Self {
            physics,
            ticker,
            locals: Default::default(),
            effects: Default::default(),
            speed: Default::default(),
        }
    }

    /// Advances one frame and reports the completed tick's processed sprint state.
    fn step(&mut self, frame: super::super::PhysicsFrameInput) -> bool {
        assert!(self.locals.advance(
            frame,
            &mut self.physics,
            &mut self.ticker,
            &mut self.effects,
            &mut self.speed,
            &VersionedFloor(1),
        ));
        self.physics
            .sample_at(self.ticker.completed_tick())
            .unwrap()
            .processed
            .sprinting
    }
}

/// A forward walking frame under the given control modes.
fn walk(
    toggle_sprint: bool,
    always_sprint: bool,
    sprint: semantic_input::ActionPhase,
) -> super::super::PhysicsFrameInput {
    super::super::PhysicsFrameInput {
        toggle_sprint,
        always_sprint,
        ..super::speed_frame(sprint)
    }
}

/// Starts a sprint with a sprint-key tap (toggle) or always-sprint and walks it in.
fn established(toggle_sprint: bool, always_sprint: bool) -> Player {
    let mut player = Player::new();
    if toggle_sprint {
        let tap = semantic_input::ActionPhase {
            pressed: true,
            held: true,
            released: false,
        };
        player.step(walk(true, false, tap));
        player.step(walk(
            true,
            false,
            semantic_input::ActionPhase {
                released: true,
                ..Default::default()
            },
        ));
    }
    for frame in 0..3 {
        assert!(
            player.step(walk(toggle_sprint, always_sprint, Default::default())),
            "control: frame {frame} did not sprint (toggle={toggle_sprint}, always={always_sprint})"
        );
    }
    player
}

/// A personal mod supplying the same forward vector, as Null Movement does while opposing keys
/// are held, must not end a sprint the player's toggle or always-sprint mode keeps.
#[test]
fn mod_movement_vector_keeps_a_toggled_or_always_sprint() {
    let mut ended = Vec::new();
    for (toggle_sprint, always_sprint) in [(true, false), (false, true)] {
        let mut player = established(toggle_sprint, always_sprint);
        for frame in 0..3 {
            let mut overridden = walk(toggle_sprint, always_sprint, Default::default());
            overridden.movement_override = true;
            if !player.step(overridden) {
                ended.push(format!(
                    "toggle={toggle_sprint} always={always_sprint} frame={frame}"
                ));
            }
        }
    }
    assert!(
        ended.is_empty(),
        "override frames ended the sprint: {ended:?}"
    );
}
