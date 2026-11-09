//! Local movement frame coordination before ordered interaction admission.
use super::control_modes::{ControlModes, ControlObservation};
use super::local_facts::LocalMovementFacts;
use super::physics_authority_fault_for_frame;
use super::{
    LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController, ModeIntent,
    MovementTicker, PhysicsSampleContext, physics_movement_input,
};
use protocol::ClientInputLock;
use semantic_input::ActionPhase;
use std::time::Duration;
use tracing::debug;

/// Vanilla actor yaw: degrees wrapped to `[-180, 180)` with f32 `fmod`.
#[must_use]
pub fn wire_yaw(degrees: f32) -> f32 {
    let rem = (degrees + 180.0) % 360.0;
    let wrapped = if rem < 0.0 { rem + 360.0 } else { rem };
    wrapped - 180.0
}

/// Vanilla head yaw keeps the unwrapped camera angle, which lies in `(-270, 90]`.
#[must_use]
pub fn wire_head_yaw(yaw: f32) -> f32 {
    if yaw > 90.0 { yaw - 360.0 } else { yaw }
}

/// Vanilla clamps each render frame's elapsed time to this and loses the rest,
/// so a stall runs at most two ticks instead of catching up.
const MAX_FRAME_ELAPSED: Duration = simulation_clock::MAX_FRAME_ELAPSED;

/// Whether a render frame of `delta` will simulate at least one fixed tick: the one answer
/// for the frame, which every system that consumes tick-bound state reads. A dismount
/// waiting for seated movement holds the frame's ticks ([`LocomotionState::holds_ticks`]).
pub fn frame_simulates_tick(
    physics: &LocalPhysicsController,
    ticker: &MovementTicker,
    locomotion: &LocomotionState,
    delta: Duration,
) -> bool {
    physics.is_active()
        && ticker.can_advance_physics_frame()
        && !locomotion.holds_ticks()
        && physics.ticks_due(delta.min(MAX_FRAME_ELAPSED)) > 0
}

/// Immutable input and modifier facts sampled at the existing physics phase.
pub struct PhysicsFrameInput {
    pub delta: Duration,
    pub now: Duration,
    pub active: bool,
    /// Cancels retained presses when the semantic router releases input ownership.
    pub input_authority_released: bool,
    /// Negotiated mod controls resolved the buttons and bypass physical toggle latches.
    pub resolved_movement: bool,
    /// A personal mod supplied the walking vector for this frame; its buttons count as held
    /// bindings, so the player's control modes still apply.
    pub movement_override: bool,
    pub movement: [f32; 2],
    pub raw_movement: [f32; 2],
    pub analogue_movement: [f32; 2],
    pub movement_buttons: semantic_input::MovementButtons,
    pub yaw: f32,
    pub pitch: f32,
    pub camera_orientation: [f32; 3],
    pub input_mode: protocol::PlayerInputMode,
    pub jump: ActionPhase,
    pub sprint: ActionPhase,
    pub sneak: ActionPhase,
    pub toggle_sprint: bool,
    pub always_sprint: bool,
    pub toggle_sneak: bool,
    pub facts: LocalMovementFacts,
    pub item_use_modifier: Option<f64>,
    pub hold: Option<PhysicsFrameHold>,
}

/// Loading freezes movement; spawn search also suppresses wire input.
#[derive(Clone, Copy)]
pub struct PhysicsFrameHold {
    pub registry: sim::CollisionRegistryIdentity,
    pub withhold_input: bool,
}

/// Persistent movement-mode input latches and collision-blocker diagnostics.
#[derive(Default)]
pub struct LocomotionState {
    controls: ControlModes,
    /// Last active device; suspended input keeps reporting it.
    input_mode: protocol::PlayerInputMode,
    previous_blocker: Option<String>,
    /// A deferred press belongs to one session and one vehicle.
    pending_dismount: Option<PendingDismount>,
    /// This frame runs no ticks while a requested dismount waits for seated movement already
    /// produced; [`Self::decide_frame_ticks`] sets it once per frame, before anything that
    /// depends on whether a tick runs.
    holds_ticks: bool,
}

/// A dismount press waiting to get the player off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingDismount {
    session: u64,
    mount: u64,
    trigger: ClientInputLock,
    /// The client places the player when it gets off this mount.
    client_places: bool,
}

/// Interaction work outside movement that may still report the seat once a stop-riding
/// request goes out.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SeatedInteractions {
    /// An attack pressed this frame, which reports the position before the frame's first tick.
    pub attack_pressed: bool,
    /// An attack press, or the block work it carries, still waiting to go out.
    pub attack_queued: bool,
    /// An item use's release still waiting to go out.
    pub item_release: bool,
    /// A block-use press still waiting to resolve.
    pub block_use: bool,
}

impl LocomotionState {
    /// Revokes retained mode requests and dismounts, including their triggering category.
    pub fn revoke_input_requests(&mut self, locks: protocol::ClientInputLocks) {
        self.controls.revoke_input_requests(locks);
        if self.pending_dismount.is_some_and(|pending| {
            locks.blocks(ClientInputLock::Dismount) || locks.blocks(pending.trigger)
        }) {
            self.pending_dismount = None;
        }
    }

    /// Admits a requested dismount ahead of movement, retaining it only on a full queue.
    pub fn flush_dismount(
        &mut self,
        position: [f32; 3],
        mut send: impl FnMut(protocol::Packet) -> Result<(), protocol::BatchSendError>,
    ) {
        let Some(PendingDismount { mount, .. }) = self.pending_dismount else {
            return;
        };
        let Ok(packet) = protocol::stop_riding_packet(mount, position) else {
            self.pending_dismount = None;
            return;
        };
        if send(packet) != Err(protocol::BatchSendError::Full) {
            self.pending_dismount = None;
        }
    }
    /// Clears input latches when local prediction is inactive.
    pub fn reset(&mut self) {
        self.controls.reset();
        self.pending_dismount = None;
        self.holds_ticks = false;
    }

    /// Decides once per frame, before anything that depends on whether a tick runs, whether
    /// this frame's ticks wait for a requested dismount: a press the client places waits while
    /// seated movement already produced is still going out, so no further seated movement
    /// queues behind it and the player's input does not move the mount meanwhile. A press
    /// first seen this frame waits from the next frame.
    pub fn decide_frame_ticks(&mut self, movement_ticker: &MovementTicker) {
        self.holds_ticks = self.pending_dismount.is_some_and(|pending| {
            pending.client_places && pending.session == movement_ticker.session_generation
        }) && Self::seated_movement_draining(movement_ticker);
    }

    /// Whether this frame's ticks wait for a requested dismount, as decided at the frame's
    /// start.
    #[must_use]
    pub const fn holds_ticks(&self) -> bool {
        self.holds_ticks
    }

    /// Retains a sneak press to leave `mount` in `session` that the client places.
    #[cfg(any(test, feature = "test-support"))]
    pub fn testing_request_dismount(&mut self, session: u64, mount: u64) {
        self.pending_dismount = Some(PendingDismount {
            session,
            mount,
            trigger: ClientInputLock::Sneak,
            client_places: true,
        });
    }

    /// Gets the player off the mount a pending press leaves, as vanilla's client does unless
    /// the server owns the mount's dismounts: just before the frame's first tick, the player
    /// stands where [`sim::Dismount`] puts it and `send` reports that position in the
    /// stop-riding request; the server accepts it. Call before [`Self::advance`] runs the
    /// frame.
    ///
    /// The player gets off only once [`Self::seated_work_outstanding`] finds nothing from the
    /// seat still on its way and transport takes the request, so the server sees everything
    /// from the seat, the request, then movement and interactions off the mount. Until then
    /// it stays seated and the press waits for a later tick; a request transport refuses for
    /// good drops the press. While seated movement is still going out, later frames run no
    /// ticks ([`Self::decide_frame_ticks`]), so a steady stream of new seated movement cannot
    /// hold the request back.
    ///
    /// Once the player gets off, `frame` no longer names the mount, and the caller tells the
    /// client world, whose mount state then keeps it off until the server's links for the
    /// player change.
    pub fn leave_mount(
        &mut self,
        frame: &mut PhysicsFrameInput,
        physics: &mut LocalPhysicsController,
        movement_ticker: &MovementTicker,
        world: &impl sim::CollisionWorld,
        interactions: SeatedInteractions,
        send: impl FnOnce(protocol::Packet) -> Result<(), protocol::BatchSendError>,
    ) -> bool {
        let session = movement_ticker.session_generation;
        self.observe_dismount(frame, session);
        let Some(PendingDismount { mount, .. }) = self
            .pending_dismount
            .filter(|pending| pending.session == session)
        else {
            return false;
        };
        let facts = frame.facts;
        let Some(placement) = facts.mount.filter(|_| facts.client_places_dismount()) else {
            return false;
        };
        if facts.ride.is_none()
            || facts.mount_runtime_id != Some(mount)
            || frame.hold.is_some()
            || !frame_simulates_tick(physics, movement_ticker, self, frame.delta)
            || self.seated_work_outstanding(movement_ticker, interactions)
        {
            return false;
        }
        let vec3 = |[x, y, z]: [f32; 3]| sim::Vec3::new(f64::from(x), f64::from(y), f64::from(z));
        // A boat the player steers moves as the client predicts it, unless the server placed
        // it since; then the boat and its seat stand where the server put them. New motion
        // from the server only steers its next tick, so the predicted pose stays current.
        let feed = physics
            .predicted_vehicle_feed()
            .filter(|feed| feed.runtime_id == mount);
        let current = feed.filter(|feed| {
            facts
                .vehicle
                .is_some_and(|vehicle| vehicle.revision.pose == feed.revision.pose)
        });
        // Other mounts move with the actor stream, so the streamed seat is the current one.
        let seat = if current.is_some() {
            physics.state().map(|state| state.position)
        } else {
            facts
                .ride_seat
                .map(vec3)
                .or_else(|| physics.state().map(|state| state.position))
        };
        let Some(seat) = seat else {
            return false;
        };
        let (position, motion) = current.map_or((placement.position, placement.motion), |feed| {
            (feed.position, feed.velocity)
        });
        let [width, height] = physics.body_size();
        // Blocks still streaming in around the mount keep the player seated until they arrive.
        let Some(exit) = (sim::Dismount {
            vehicle: vec3(position),
            vehicle_motion: vec3(motion),
            seat,
            rider_width: f64::from(width),
            rider_height: f64::from(height),
        })
        .exit_feet(world) else {
            return false;
        };
        let position = [
            exit.x as f32,
            exit.y as f32 + protocol::PLAYER_NETWORK_OFFSET,
            exit.z as f32,
        ];
        let Ok(packet) = protocol::stop_riding_packet(mount, position) else {
            self.pending_dismount = None;
            return false;
        };
        match send(packet) {
            Ok(()) => self.pending_dismount = None,
            Err(protocol::BatchSendError::Full) => return false,
            Err(protocol::BatchSendError::Closed) => {
                self.pending_dismount = None;
                return false;
            }
        }
        physics.apply_server_actor_position(position, None);
        // Corrections for the boat the player steered no longer move the player.
        if feed.is_some() {
            physics.end_vehicle_prediction();
        }
        frame.facts = frame.facts.off_mount();
        true
    }

    /// Whether movement from the seat is still on its way without needing another tick, which
    /// is what a waiting dismount holds the ticks for; interaction work, blocked ticks and a
    /// release facing a tick still to come may need further ticks, so they never hold them:
    /// queued movement, movement sent but not yet written to the socket (transport may cancel
    /// it and it goes out again), or a held item release behind a seated tick already produced.
    fn seated_movement_draining(movement_ticker: &MovementTicker) -> bool {
        movement_ticker.pending_count() > 0 || movement_ticker.held_release_awaits_written_tick()
    }

    /// Whether anything from the seat could still reach the server after a stop-riding
    /// request sent now, or the player's place is not settled: seated movement still going
    /// out, a held item release, interaction work still waiting, or a last tick that waited
    /// for collision data.
    fn seated_work_outstanding(
        &self,
        movement_ticker: &MovementTicker,
        interactions: SeatedInteractions,
    ) -> bool {
        let SeatedInteractions {
            attack_pressed,
            attack_queued,
            item_release,
            block_use,
        } = interactions;
        Self::seated_movement_draining(movement_ticker)
            || movement_ticker.has_held_release()
            || attack_pressed
            || attack_queued
            || item_release
            || block_use
            || self.previous_blocker.is_some()
    }

    /// Runs each fixed tick and admits its sample before any interaction producers run.
    pub fn advance(
        &mut self,
        frame: PhysicsFrameInput,
        physics: &mut LocalPhysicsController,
        movement_ticker: &mut MovementTicker,
        movement_effects: &mut LocalMovementEffectTimeline,
        movement_speed: &mut LocalMovementSpeedAuthority,
        world: &impl sim::CollisionWorld,
    ) -> bool {
        movement_ticker.settle_riptide_input(physics);
        movement_effects.begin_frame();
        self.observe_dismount(&frame, movement_ticker.session_generation);
        let PhysicsFrameInput {
            now,
            active,
            movement,
            raw_movement,
            analogue_movement,
            yaw,
            input_mode,
            jump,
            sprint,
            sneak,
            facts,
            ..
        } = frame;
        if active {
            self.input_mode = input_mode;
        }
        let input_mode = self.input_mode;
        let movement = facts.input_locks.movement(movement);
        let sprint_allowed = !facts.input_locks.blocks(ClientInputLock::MoveForward);
        let processed_sneak = ActionPhase {
            pressed: sneak.pressed && !facts.input_locks.blocks(ClientInputLock::Sneak),
            held: sneak.held && !facts.input_locks.blocks(ClientInputLock::Sneak),
            released: sneak.released,
        };
        let head_yaw = wire_head_yaw(yaw);
        let delta = frame.delta.min(MAX_FRAME_ELAPSED);
        let withhold_input = frame.hold.is_some_and(|hold| hold.withhold_input);
        let requested_speed;
        let frame = if let Some(hold) = frame.hold {
            self.reset();
            movement_speed.set_sprinting(false);
            requested_speed = movement_speed.prediction_speed();
            physics.advance_dimension_wait(
                delta,
                yaw,
                PhysicsSampleContext {
                    pitch: frame.pitch,
                    head_yaw,
                    camera_orientation: frame.camera_orientation,
                    input_mode,
                    ..PhysicsSampleContext::default()
                },
                hold.registry,
                movement_effects,
            )
        } else {
            if !active {
                self.controls.suspend(input_mode.persists_sneak());
            }
            let server = physics.take_server_control_flags();
            if let Some(server) = server {
                movement_speed.adopt_server_sprinting(server.sprinting);
                self.controls
                    .adopt_server_flags(server.sprinting, server.sneaking);
            }
            let retain_sprint = physics
                .retains_swim_sprint(world)
                .unwrap_or_else(|_| physics.mode() == sim::MovementMode::Swimming);
            let movement_override =
                frame.movement_override && active && !frame.input_authority_released;
            let sprint_direction = if movement_override {
                [0.0, movement[0].hypot(movement[1])]
            } else {
                movement
            };
            let controlled = self.controls.update(ControlObservation {
                now,
                resolved_movement: frame.resolved_movement,
                forward: sprint_direction[1],
                sideways: sprint_direction[0],
                touch_input: input_mode == protocol::PlayerInputMode::Touch,
                sprint_pressed: sprint.pressed && sprint_allowed,
                sprint_held: sprint.held && sprint_allowed,
                sneak_pressed: processed_sneak.pressed,
                sneak_held: processed_sneak.held,
                toggle_sprint: frame.toggle_sprint,
                always_sprint: active && frame.always_sprint && sprint_allowed,
                toggle_sneak: frame.toggle_sneak,
                sprint_blocked: facts.sprint_blocked,
                sprint_start_blocked: facts.sprint_start_blocked,
                flying: physics.mode() == sim::MovementMode::Flying,
                retain_sprint,
                hold_sneak: !active && input_mode.persists_sneak(),
            });
            let mut input = physics_movement_input(
                movement,
                yaw,
                active,
                jump.held,
                controlled.sneaking && !facts.input_locks.blocks(ClientInputLock::Sneak),
                controlled.sprint_request,
                frame.item_use_modifier,
            );
            if retain_sprint || movement_override {
                input.sprinting = controlled.sprint_request;
            }
            if !active {
                input.sneaking =
                    controlled.sneaking && !facts.input_locks.blocks(ClientInputLock::Sneak);
            }
            input.immobile = facts.immobile;
            input.vertical_physics = facts.vertical_physics;
            input.movement_speed = movement_speed.requested_prediction_speed(input.sprinting);
            let liquid = movement_speed.liquid();
            input.underwater_movement_speed = liquid.underwater;
            input.lava_movement_speed = liquid.lava;
            requested_speed = input.movement_speed;
            physics.advance_with_context_and_effects(
                delta,
                input,
                PhysicsSampleContext {
                    pitch: frame.pitch,
                    head_yaw,
                    camera_orientation: frame.camera_orientation,
                    input_mode,
                    raw_move_vector: raw_movement,
                    analogue_move_vector: analogue_movement,
                    mode_intent: ModeIntent {
                        movement_override,
                        spectator: facts.spectator,
                        no_clip: facts.no_clip,
                        ride: facts.ride,
                        ride_seat: facts.ride_seat,
                        vehicle: facts.vehicle,
                        vehicle_controlled_locally: facts.vehicle_controlled_locally,
                        can_fly: facts.can_fly,
                        flight: facts.flight,
                        ability_sequence: facts.ability_sequence,
                        fly_speed: facts.fly_speed,
                        vertical_fly_speed: facts.vertical_fly_speed,
                        creative_flight: facts.creative_flight,
                        elytra_ready: facts.elytra_ready,
                        instabuild: facts.instabuild,
                        can_stand_on_snow: facts.can_stand_on_snow,
                        depth_strider: facts.depth_strider,
                        soul_speed: facts.soul_speed,
                        swift_sneak: facts.swift_sneak,
                        swim_hunger_blocked: facts.swim_hunger_blocked,
                        sprint_blocked: facts.sprint_blocked,
                        sprint_start_blocked: facts.sprint_start_blocked,
                        stop_sprinting: controlled.stop_sprinting,
                    },
                    release_use_modifier: movement_ticker.release_use_modifier(),
                    input_locks: facts.input_locks,
                    input: super::TickInput {
                        movement_buttons: filtered_movement_buttons(
                            frame.movement_buttons,
                            facts.input_locks,
                        ),
                        jump,
                        sneak: processed_sneak,
                        sprint_down: controlled.sprint_down,
                        sneak_down: input.sneaking,
                        // A gamepad's sneak control is vanilla's toggle-sneak
                        // control under either sneak setting.
                        sneak_toggle_down: (frame.toggle_sneak
                            || input_mode == protocol::PlayerInputMode::GamePad)
                            && processed_sneak.held,
                        server_sprinting: server.and_then(|flags| flags.sprinting),
                        server_sneaking: server.and_then(|flags| flags.sneaking),
                        server_mode: None,
                        server_pose_flags: None,
                        server_spinning: None,
                    },
                },
                world,
                movement_effects,
            )
        };
        if let Some(sample) = frame.samples.last() {
            self.controls
                .adopt_tick_sprinting(sample.processed.sprinting);
            movement_speed.set_sprinting(sample.processed.sprinting);
        }
        super::control_trace::trace_physics_frame(
            movement_ticker.session_generation,
            now,
            requested_speed,
            movement_speed.current(),
            &frame,
        );
        let blocker = frame.blocked.as_ref().map(ToString::to_string);
        if frame.dropped_ticks != 0 {
            // Time starvation keeps the retained samples contiguous and monotonic,
            // so the outbound stream remains a valid 20 Hz sequence and the server
            // can still reconcile it. Record the stall instead of permanently
            // revoking authority over a load symptom; a live Venity session died
            // exactly here when join-time streaming stalls dropped seven of
            // fifteen due ticks.
            debug!(
                due = frame.due_ticks,
                dropped = frame.dropped_ticks,
                "local physics dropped excess catch-up ticks"
            );
        }
        let authority_fault = physics_authority_fault_for_frame(&frame);
        if blocker != self.previous_blocker {
            if authority_fault.is_none()
                && let Some(blocker) = blocker.as_deref()
            {
                debug!(%blocker, "local physics is waiting for collision data or a bounded query");
            }
            self.previous_blocker = blocker;
        }
        if let Some(fault) = authority_fault
            && movement_ticker.physics_is_authorized()
        {
            movement_ticker.record_physics_fault(fault);
            physics.deactivate();
            return false;
        }
        for sample in frame.samples {
            let admitted = if withhold_input {
                movement_ticker.withhold_respawn_input(sample)
            } else {
                movement_ticker.enqueue_completed_physics_at(sample, Some(now))
            };
            if let Err(fault) = admitted {
                debug!(?fault, "local Physics movement authority failed closed");
                physics.deactivate();
                return false;
            }
        }
        true
    }

    /// Retains riding input even when transport admission pauses physics, preserving server links.
    pub fn observe_dismount(&mut self, frame: &PhysicsFrameInput, session: u64) {
        // A boat maps both jump and sneak to dismount; other mounts keep jump for themselves.
        // A press counts through whichever pressed button its locks still allow.
        let boat = frame.facts.ride == Some(super::RideKind::Boat);
        let allowed = |lock| !frame.facts.input_locks.blocks(lock);
        let trigger = [
            (boat && frame.jump.pressed, ClientInputLock::Jump),
            (frame.sneak.pressed, ClientInputLock::Sneak),
        ]
        .into_iter()
        .find(|(pressed, lock)| *pressed && allowed(*lock))
        .map(|(_, lock)| lock);
        let mount = frame
            .facts
            .ride
            .and(frame.facts.mount_runtime_id)
            .filter(|mount| *mount != 0)
            .filter(|_| frame.active && !frame.input_authority_released && frame.hold.is_none())
            .map(|mount| (session, mount));
        // A retained press survives later frames on the same mount, whichever button made it.
        if self
            .pending_dismount
            .map(|pending| (pending.session, pending.mount))
            != mount
        {
            self.pending_dismount = None;
        }
        // Who places the player can change while the press waits.
        if let Some(pending) = &mut self.pending_dismount {
            pending.client_places = frame.facts.client_places_dismount();
        }
        if let (Some(trigger), Some((session, mount))) = (trigger, mount)
            && allowed(ClientInputLock::Dismount)
        {
            self.pending_dismount = Some(PendingDismount {
                session,
                mount,
                trigger,
                client_places: frame.facts.client_places_dismount(),
            });
        }
    }
}

/// Filters digital directions independently of raw and analogue device vectors.
fn filtered_movement_buttons(
    buttons: semantic_input::MovementButtons,
    locks: protocol::ClientInputLocks,
) -> semantic_input::MovementButtons {
    semantic_input::MovementButtons {
        forward: buttons.forward && !locks.blocks(ClientInputLock::MoveForward),
        backward: buttons.backward && !locks.blocks(ClientInputLock::MoveBackward),
        left: buttons.left && !locks.blocks(ClientInputLock::MoveLeft),
        right: buttons.right && !locks.blocks(ClientInputLock::MoveRight),
    }
}
