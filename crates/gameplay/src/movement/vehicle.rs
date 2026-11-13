//! Client prediction of the boat the local player steers.
//!
//! Each tick the rider's move input paddles the boat locally and the rider sits on its seat
//! at the predicted pose. Inputs then report the boat's position and rotation. A server
//! correction of the vehicle describes the boat at an earlier input tick, so prediction
//! restarts from the corrected state and replays the inputs recorded since, as vanilla does.

use std::collections::VecDeque;

use sim::{BoatBody, BoatBuoyancy, BoatState, CollisionWorld, WorldQueryError};

/// The boat the local player steers from its controlling seat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SteeredVehicle {
    pub runtime_id: u64,
    pub unique_id: i64,
    /// Collision box and position origin from the boat's actor facts.
    pub body: BoatBody,
    /// Network position, yaw and per-tick motion the boat retains; prediction starts there.
    pub position: [f32; 3],
    pub yaw: f32,
    pub velocity: [f32; 3],
    /// The rider's seat relative to the boat's feet, in the boat's frame.
    pub seat: [f32; 3],
    /// Server changes to the boat: a new pose revision restarts prediction, a new motion
    /// revision replaces its motion.
    pub revision: client_world::VehicleRevision,
}

/// The predicted vehicle reported in one tick's input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PredictedVehicleSample {
    pub unique_id: i64,
    /// Network position.
    pub position: [f32; 3],
    /// Displacement applied next tick.
    pub velocity: [f32; 3],
    /// Pitch and yaw in degrees.
    pub rotation: [f32; 2],
}

impl PredictedVehicleSample {
    /// The vehicle fields of the input that reports this tick.
    #[must_use]
    pub const fn input(&self) -> protocol::PredictedVehicleInput {
        protocol::PredictedVehicleInput {
            unique_id: self.unique_id,
            position: self.position,
            delta: self.velocity,
            rotation: self.rotation,
        }
    }
}

/// A server correction of the predicted vehicle at one input tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VehicleCorrection {
    /// The vehicle the player steered when the correction arrived.
    pub runtime_id: u64,
    /// The vehicle revision when the correction arrived; the corrected session continues
    /// from it, and only server changes after the correction apply on top.
    pub revision: client_world::VehicleRevision,
    pub tick: u64,
    /// Network position.
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    /// Pitch and yaw in degrees.
    pub rotation: [f32; 2],
    pub angular_velocity: Option<f32>,
    pub on_ground: bool,
}

/// Where a corrected boat seats its rider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorrectedSeat {
    /// Seat feet on the boat's latest tick.
    pub seat: [f32; 3],
    /// Seat feet one tick before it, which rendering eases from.
    pub previous: [f32; 3],
}

/// A render-only shift of the boat's pose.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct PoseShift {
    position: [f32; 3],
    yaw: f32,
}

impl PoseShift {
    /// The shift `alpha` of the way from `self` to `to`.
    fn toward(self, to: Self, alpha: f32) -> Self {
        Self {
            position: std::array::from_fn(|axis| {
                self.position[axis] + (to.position[axis] - self.position[axis]) * alpha
            }),
            yaw: self.yaw + (to.yaw - self.yaw) * alpha,
        }
    }
}

/// What the render of a boat still owes a server correction. A correction moves the boat's
/// latest and previous poses; the shift keeps each rendered pose where it was and fades out
/// over the next ticks, so the boat and the view that follows it do not jump.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct RenderBlend {
    previous: PoseShift,
    current: PoseShift,
}

impl RenderBlend {
    /// The share of the shift that is left after each tick.
    const KEPT_PER_TICK: f32 = 0.5;

    /// Adds the shift that keeps the latest and previous poses rendering where they did.
    fn add(&mut self, current: PoseShift, previous: PoseShift) {
        for (shift, add) in [(&mut self.current, current), (&mut self.previous, previous)] {
            for axis in 0..3 {
                shift.position[axis] += add.position[axis];
            }
            shift.yaw += add.yaw;
        }
    }

    /// Fades the shift for the next tick, rendering on from the one that just ended.
    fn tick(&mut self) {
        self.previous = self.current;
        let kept = Self::KEPT_PER_TICK;
        self.current.position = self.current.position.map(|axis| axis * kept);
        self.current.yaw *= kept;
    }

    /// Holds the shift where the tick left it while no tick follows.
    fn finish_interpolation(&mut self) {
        self.previous = self.current;
    }

    fn at(&self, alpha: f32) -> PoseShift {
        self.previous.toward(self.current, alpha)
    }
}

/// Input ticks kept for replaying server corrections, about six seconds.
const HISTORY_TICKS: usize = 120;

/// What one committed tick fed the boat and left it as, kept to rewind to that tick and replay
/// the ticks after it when the server corrects the boat.
#[derive(Debug, Clone, PartialEq)]
struct TickInput {
    tick: u64,
    move_vector: [f32; 2],
    wave_draw: f32,
    /// The boat after the tick, including the paddle and wave state a correction does not carry.
    after: BoatState,
}

/// Identifies one prediction session: the steered boat and the pose revision it started from.
/// A change of owner starts a new session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SessionOwner {
    runtime_id: u64,
    unique_id: i64,
    pose_revision: u64,
}

#[derive(Debug, Clone)]
struct Session {
    owner: SessionOwner,
    seat: [f32; 3],
    state: BoatState,
    /// The motion revision the session's velocity follows.
    motion_revision: u64,
    /// The yaw one tick before `state`, which rendering eases from.
    previous_yaw: f32,
    /// The network position one tick before `state`, which rendering eases from.
    previous_position: [f32; 3],
    /// Corrections at or before this tick belong to an earlier session or are already
    /// applied; `None` until the session's first tick completes.
    stale_through: Option<u64>,
    /// What rendering still owes the corrections applied to this session.
    blend: RenderBlend,
}

#[derive(Debug, Clone, Default)]
pub(super) struct VehiclePrediction {
    /// The session as of the last committed tick.
    session: Option<Session>,
    /// The session as the current tick stepped it, installed only when the tick commits, so a
    /// failed tick leaves the committed session as it was.
    pending: Option<Session>,
    /// The input the pending session's tick stepped with.
    pending_input: Option<([f32; 2], f32)>,
    /// Inputs of the session's committed ticks, oldest first.
    history: VecDeque<TickInput>,
    /// Xorshift state for the wave's big-wave draws.
    wave_seed: u32,
}

impl VehiclePrediction {
    /// Paddles the steered boat for the coming tick and returns the rider's seat feet, or
    /// `None` without a steered boat. A new boat or server change starts a session from the
    /// boat's retained pose and motion. A boat whose terrain is unavailable fails the tick, so
    /// its input never silently reports the rider instead.
    pub(super) fn step(
        &mut self,
        vehicle: Option<SteeredVehicle>,
        move_vector: [f32; 2],
        world: &impl CollisionWorld,
    ) -> Result<Option<[f32; 3]>, WorldQueryError> {
        self.pending = None;
        self.pending_input = None;
        let Some(vehicle) = vehicle else {
            self.session = None;
            self.history.clear();
            return Ok(None);
        };
        let owner = SessionOwner {
            runtime_id: vehicle.runtime_id,
            unique_id: vehicle.unique_id,
            pose_revision: vehicle.revision.pose,
        };
        let continued = self
            .session
            .as_ref()
            .is_some_and(|session| session.owner == owner);
        if !continued {
            self.history.clear();
        }
        let mut session = match &self.session {
            Some(session) if session.owner == owner => {
                let mut session = session.clone();
                // New server motion replaces the predicted motion and keeps the pose.
                if session.motion_revision != vehicle.revision.motion {
                    session.motion_revision = vehicle.revision.motion;
                    session.state.velocity = vehicle.velocity;
                }
                session
            }
            _ => Session {
                owner,
                seat: vehicle.seat,
                state: BoatState {
                    velocity: vehicle.velocity,
                    ..BoatState::at_rest(vehicle.body, vehicle.position, vehicle.yaw)
                },
                motion_revision: vehicle.revision.motion,
                previous_yaw: vehicle.yaw,
                previous_position: vehicle.position,
                stale_through: None,
                blend: RenderBlend::default(),
            },
        };
        session.blend.tick();
        session.seat = vehicle.seat;
        session.state.body = vehicle.body;
        let wave_draw = next_draw(&mut self.wave_seed);
        let tick = sim::tick_boat(
            world,
            &session.state,
            sim::boat_paddle_power(move_vector),
            BoatBuoyancy::Live { wave_draw },
        )?;
        // A continued session renders from its last tick. A new one renders at its first
        // stepped pose, where its rider is seated, rather than easing across the restart.
        let from = if continued {
            &session.state
        } else {
            &tick.state
        };
        session.previous_yaw = from.yaw;
        session.previous_position = from.position;
        session.state = tick.state;
        let seat = seat_feet(&session.state, session.seat);
        self.pending = Some(session);
        self.pending_input = Some((move_vector, wave_draw));
        Ok(Some(seat))
    }

    /// Installs the stepped session as the state of the completed input tick.
    pub(super) fn commit(&mut self, tick: u64) -> Option<PredictedVehicleSample> {
        let mut session = self.pending.take()?;
        session.stale_through.get_or_insert(tick.saturating_sub(1));
        let sample = predicted_sample(session.owner.unique_id, &session.state);
        if let Some((move_vector, wave_draw)) = self.pending_input.take() {
            if self.history.len() == HISTORY_TICKS {
                self.history.pop_front();
            }
            self.history.push_back(TickInput {
                tick,
                move_vector,
                wave_draw,
                after: session.state.clone(),
            });
        }
        self.session = Some(session);
        Some(sample)
    }

    /// Renders the boat at its committed state until the next tick.
    pub(super) fn finish_interpolation(&mut self) {
        if let Some(session) = &mut self.session {
            session.previous_yaw = session.state.yaw;
            session.previous_position = session.state.position;
            session.blend.finish_interpolation();
        }
    }

    /// Whether this tick stepped a predicted boat.
    pub(super) const fn stepped(&self) -> bool {
        self.pending.is_some()
    }

    /// Whether this tick stepped a predicted boat on from its committed session, rather than
    /// starting it at a new pose.
    pub(super) fn continued(&self) -> bool {
        matches!(
            (&self.pending, &self.session),
            (Some(pending), Some(session)) if pending.owner == session.owner
        )
    }

    /// Discards a stepped boat whose player tick failed.
    pub(super) fn discard_pending(&mut self) {
        self.pending = None;
    }

    /// Ends the session.
    pub(super) fn reset(&mut self) {
        self.session = None;
        self.pending = None;
        self.pending_input = None;
        self.history.clear();
    }

    /// The predicted boat's yaw `alpha` of the way through the current tick.
    pub(super) fn render_yaw(&self, alpha: f32) -> Option<f32> {
        let session = self
            .session
            .as_ref()
            .filter(|session| session.stale_through.is_some())?;
        let turn = (session.state.yaw - session.previous_yaw + 180.0).rem_euclid(360.0) - 180.0;
        let alpha = alpha.clamp(0.0, 1.0);
        Some(session.previous_yaw + turn * alpha + session.blend.at(alpha).yaw)
    }

    /// The predicted boat's latest pose for the actor store, and where it renders `alpha` of
    /// the way through the current tick, on the rider's clock.
    pub(super) fn feed(&self, alpha: f32) -> Option<client_world::LocalVehicleFeed> {
        let session = self
            .session
            .as_ref()
            .filter(|session| session.stale_through.is_some())?;
        let feet = |position: [f32; 3]| {
            [
                position[0],
                position[1] - session.state.body.height_offset,
                position[2],
            ]
        };
        let alpha = alpha.clamp(0.0, 1.0);
        let shift = session.blend.at(alpha).position;
        let shown = std::array::from_fn(|axis| {
            let from = session.previous_position[axis];
            from + (session.state.position[axis] - from) * alpha + shift[axis]
        });
        Some(client_world::LocalVehicleFeed {
            runtime_id: session.owner.runtime_id,
            revision: client_world::VehicleRevision {
                pose: session.owner.pose_revision,
                motion: session.motion_revision,
            },
            position: feet(session.state.position),
            yaw: session.state.yaw,
            velocity: session.state.velocity,
            render_position: feet(shown),
            render_yaw: self.render_yaw(alpha)?,
        })
    }

    /// Continues the session from a server correction's vehicle state, replays the inputs of
    /// the ticks committed since it, and returns the rider's seat feet on the result. If the
    /// terrain a replayed tick needs is unavailable, the session stays at the corrected state. A correction of another vehicle, from before this session or its
    /// revision, no newer than one already applied, or with non-finite state is ignored.
    pub(super) fn correct(
        &mut self,
        correction: &VehicleCorrection,
        world: &impl CollisionWorld,
    ) -> Option<CorrectedSeat> {
        let session = self.session.as_mut()?;
        let finite = correction
            .position
            .iter()
            .chain(&correction.velocity)
            .chain(&correction.rotation)
            .chain(correction.angular_velocity.as_ref())
            .all(|value| value.is_finite());
        if !finite
            || correction.runtime_id != session.owner.runtime_id
            || correction.revision.pose < session.owner.pose_revision
            || correction.tick <= session.stale_through?
        {
            return None;
        }
        // The correction is newer than any server change it arrived after; changes after it
        // still apply.
        session.owner.pose_revision = correction.revision.pose;
        session.motion_revision = correction.revision.motion;
        session.stale_through = Some(correction.tick);
        let shown = (
            session.state.position,
            session.state.yaw,
            session.previous_position,
            session.previous_yaw,
        );
        // The tick before the corrected one, as the client predicted it, is what rendering
        // eases from when no replayed tick follows.
        let before = self
            .history
            .iter()
            .find(|input| input.tick + 1 == correction.tick)
            .map_or((correction.position, correction.rotation[1]), |input| {
                (input.after.position, input.after.yaw)
            });
        (session.previous_position, session.previous_yaw) = (before.0, before.1);
        // The corrected tick's own paddle and wave state, which the correction does not
        // carry, are what the replay continues from.
        let rewound = self
            .history
            .iter()
            .find(|input| input.tick == correction.tick)
            .map_or_else(|| session.state.clone(), |input| input.after.clone());
        session.state = BoatState {
            position: correction.position,
            velocity: correction.velocity,
            yaw: correction.rotation[1],
            angular_velocity: correction
                .angular_velocity
                .unwrap_or(rewound.angular_velocity),
            on_ground: correction.on_ground,
            ..rewound
        };
        self.pending = None;
        self.pending_input = None;
        self.history.retain(|input| input.tick > correction.tick);
        let mut replayed = session.state.clone();
        let mut previous = (session.previous_position, session.previous_yaw);
        let replay = self.history.iter().try_for_each(|input| {
            previous = (replayed.position, replayed.yaw);
            replayed = sim::tick_boat(
                world,
                &replayed,
                sim::boat_paddle_power(input.move_vector),
                BoatBuoyancy::Live {
                    wave_draw: input.wave_draw,
                },
            )?
            .state;
            Ok::<(), WorldQueryError>(())
        });
        if replay.is_ok() {
            session.state = replayed;
            (session.previous_position, session.previous_yaw) = previous;
        }
        let wrap = |degrees: f32| (degrees + 180.0).rem_euclid(360.0) - 180.0;
        let moved =
            |from: [f32; 3], to: [f32; 3]| std::array::from_fn(|axis| from[axis] - to[axis]);
        session.blend.add(
            PoseShift {
                position: moved(shown.0, session.state.position),
                yaw: wrap(shown.1 - session.state.yaw),
            },
            PoseShift {
                position: moved(shown.2, session.previous_position),
                yaw: wrap(shown.3 - session.previous_yaw),
            },
        );
        let before = BoatState {
            position: session.previous_position,
            yaw: session.previous_yaw,
            ..session.state.clone()
        };
        Some(CorrectedSeat {
            seat: seat_feet(&session.state, session.seat),
            previous: seat_feet(&before, session.seat),
        })
    }
}

#[cfg(test)]
impl VehiclePrediction {
    /// Predicts `vehicle` from a session whose ticks through `completed` already ran.
    pub(crate) fn completed_session(vehicle: SteeredVehicle, completed: u64) -> Self {
        Self {
            session: Some(Session {
                owner: SessionOwner {
                    runtime_id: vehicle.runtime_id,
                    unique_id: vehicle.unique_id,
                    pose_revision: vehicle.revision.pose,
                },
                seat: vehicle.seat,
                state: BoatState::at_rest(vehicle.body, vehicle.position, vehicle.yaw),
                motion_revision: vehicle.revision.motion,
                previous_yaw: vehicle.yaw,
                previous_position: vehicle.position,
                stale_through: Some(completed),
                blend: RenderBlend::default(),
            }),
            ..Self::default()
        }
    }
}

fn predicted_sample(unique_id: i64, state: &BoatState) -> PredictedVehicleSample {
    PredictedVehicleSample {
        unique_id,
        position: state.position,
        velocity: state.velocity,
        rotation: [0.0, state.yaw],
    }
}

/// Feet of a rider whose seat sits at `seat` in the boat's frame.
fn seat_feet(state: &BoatState, seat: [f32; 3]) -> [f32; 3] {
    let offset = client_world::seat_world_offset(seat, state.yaw);
    let [x, _, z] = state.position;
    [x + offset[0], state.feet_y() + offset[1], z + offset[2]]
}

/// A uniform draw in `[0, 1)` for the wave's big-wave chance.
fn next_draw(seed: &mut u32) -> f32 {
    let mut value = if *seed == 0 { 0x9e37_79b9 } else { *seed };
    value ^= value << 13;
    value ^= value >> 17;
    value ^= value << 5;
    *seed = value;
    (value >> 8) as f32 / (1_u32 << 24) as f32
}

#[cfg(test)]
#[path = "vehicle_tests.rs"]
mod tests;
