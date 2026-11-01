//! Bedrock's movement interpolation of a Normal-mode `MovePlayer` for the local player.

use super::*;

/// One in-flight ease toward a Normal-mode `MovePlayer` target, in network-position space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct MovementLerp {
    target: [f32; 3],
    remaining: u32,
}

impl MovementLerp {
    /// Runs one interpolation step at the start of a tick, before travel, and returns the
    /// ease still in flight afterwards.
    ///
    /// As in Bedrock, every step clears velocity and closes `1/remaining` of the gap in the
    /// packet's float precision; the last step lands exactly on the target.
    pub(super) fn step(self, state: &mut PlayerState) -> Option<Self> {
        state.velocity = Vec3::ZERO;
        let position = if self.remaining > 1 {
            let fraction = 1.0 / self.remaining as f32;
            let axis = |current: f32, target: f32| current + (target - current) * fraction;
            [
                axis(state.position.x as f32, self.target[0]),
                axis(
                    state.position.y as f32 + PLAYER_NETWORK_OFFSET,
                    self.target[1],
                ),
                axis(state.position.z as f32, self.target[2]),
            ]
        } else {
            self.target
        };
        state.set_position(Vec3::new(
            f64::from(position[0]),
            f64::from(position[1] - PLAYER_NETWORK_OFFSET),
            f64::from(position[2]),
        ));
        let remaining = self.remaining.saturating_sub(1);
        (remaining > 0).then_some(Self { remaining, ..self })
    }
}

impl LocalPhysicsController {
    /// Starts easing toward a Normal-mode `MovePlayer` target over Bedrock's interpolation
    /// steps instead of rewinding or snapping, adopting the packet's ground flag at once.
    ///
    /// A newer target replaces one in flight. Returns `false` and changes nothing without
    /// local state, for a non-finite target, or when the target's terrain is not resident,
    /// which Bedrock places directly instead of easing.
    pub fn start_server_movement_lerp(
        &mut self,
        network_position: [f32; 3],
        on_ground: bool,
        world: &impl CollisionWorld,
    ) -> bool {
        if !network_position.into_iter().all(f32::is_finite) {
            return false;
        }
        let Some(state) = self.state.as_mut() else {
            return false;
        };
        let target = Vec3::new(
            f64::from(network_position[0]),
            f64::from(network_position[1] - PLAYER_NETWORK_OFFSET),
            f64::from(network_position[2]),
        );
        if world.ensure_movement_origin(target).is_err() {
            return false;
        }
        state.on_ground = on_ground;
        self.movement_lerp = Some(MovementLerp {
            target: network_position,
            remaining: client_world::ACTOR_INTERPOLATION_TICKS,
        });
        true
    }
}
