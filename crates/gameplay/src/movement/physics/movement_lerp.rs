//! Bedrock's movement interpolation of a Normal-mode `MovePlayer` for the local player.

use super::*;

/// A native height and the collision feet it produced, before an offset round trip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct NetworkHeight {
    native: f32,
}

impl NetworkHeight {
    /// Retains the exact network height alongside its collision origin.
    pub(super) fn new(native: f32) -> Self {
        Self { native }
    }

    /// Converts the retained height to the collision origin once in packet precision.
    pub(super) fn feet(self) -> f64 {
        f64::from(self.native - PLAYER_NETWORK_OFFSET)
    }

    /// Preserves the native height while travel leaves its feet height unchanged.
    pub(super) fn at(self, feet: f64) -> f32 {
        if self.matches(feet) {
            self.native
        } else {
            feet as f32 + PLAYER_NETWORK_OFFSET
        }
    }

    /// Whether travel has kept the collision height represented by this native value.
    pub(super) fn matches(self, feet: f64) -> bool {
        feet == self.feet()
    }

    /// Encodes collision feet without rounding an unchanged native height twice.
    pub(super) fn position(height: Option<Self>, feet: Vec3) -> [f32; 3] {
        [
            feet.x as f32,
            height.map_or_else(|| feet.y as f32 + PLAYER_NETWORK_OFFSET, |h| h.at(feet.y)),
            feet.z as f32,
        ]
    }
}

/// One in-flight ease toward a Normal-mode `MovePlayer` target, in network-position space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct MovementLerp {
    target: [f32; 3],
    remaining: u32,
    ground_write: Option<bool>,
    rotation: Option<[f32; 2]>,
}

impl MovementLerp {
    /// Reapplies a ground write that has not yet reached a completed tick.
    pub(super) fn apply_ground_write(self, state: &mut PlayerState) {
        if let Some(on_ground) = self.ground_write {
            state.on_ground = on_ground;
        }
    }

    /// Lets a later actor placement own the ground state at this boundary.
    pub(super) fn supersede_ground_write(&mut self) {
        self.ground_write = None;
    }

    /// Reapplies the packet boundary before stepping a retained frame.
    pub(super) fn replay(self, state: &mut PlayerState, height: &mut Option<NetworkHeight>) {
        self.apply_ground_write(state);
        self.step(state, height);
    }

    /// Advances the physical aim with the position's remaining-step counter.
    pub(super) fn step_rotation(self, current: [f32; 2]) -> Option<[f32; 2]> {
        let target = self.rotation?;
        Some(std::array::from_fn(|axis| {
            let next = if self.remaining > 1 {
                current[axis]
                    + crate::movement::wire_yaw(target[axis] - current[axis])
                        * (1.0 / self.remaining as f32)
            } else {
                target[axis]
            };
            crate::movement::wire_yaw(next)
        }))
    }

    /// Runs one interpolation step at the start of a tick, before travel, and returns the
    /// ease still in flight afterwards.
    ///
    /// As in Bedrock, every step clears velocity and closes `1/remaining` of the gap in the
    /// packet's float precision; the last step lands exactly on the target.
    pub(super) fn step(
        self,
        state: &mut PlayerState,
        height: &mut Option<NetworkHeight>,
    ) -> Option<Self> {
        state.velocity = Vec3::ZERO;
        let position = if self.remaining > 1 {
            let fraction = 1.0 / self.remaining as f32;
            let axis = |current: f32, target: f32| current + (target - current) * fraction;
            [
                axis(state.position.x as f32, self.target[0]),
                axis(
                    NetworkHeight::position(*height, state.position)[1],
                    self.target[1],
                ),
                axis(state.position.z as f32, self.target[2]),
            ]
        } else {
            self.target
        };
        let next_height = NetworkHeight::new(position[1]);
        state.set_position(Vec3::new(
            f64::from(position[0]),
            next_height.feet(),
            f64::from(position[2]),
        ));
        *height = Some(next_height);
        let remaining = self.remaining.saturating_sub(1);
        (remaining > 0).then_some(Self {
            remaining,
            ground_write: None,
            ..self
        })
    }
}

impl LocalPhysicsController {
    /// Discards an ease and its retained steps when a teleport supersedes them.
    pub(in crate::movement) fn discard_server_movement_lerp(&mut self) {
        self.movement_lerp = None;
        self.eased_rotation = None;
        for frame in &mut self.controller_history {
            frame.movement_lerp = None;
        }
    }

    /// Sets the filtered rotation target without turning the view at packet arrival.
    pub fn set_server_movement_lerp_rotation(&mut self, rotation: [f32; 2]) {
        if rotation.into_iter().all(f32::is_finite)
            && let Some(lerp) = self.movement_lerp.as_mut()
        {
            lerp.rotation = Some(rotation);
        }
    }

    /// Takes the last successfully eased physical aim for publication to the view.
    pub fn take_eased_rotation(&mut self) -> Option<[f32; 2]> {
        self.eased_rotation.take()
    }

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
            ground_write: Some(on_ground),
            rotation: None,
        });
        true
    }
}
