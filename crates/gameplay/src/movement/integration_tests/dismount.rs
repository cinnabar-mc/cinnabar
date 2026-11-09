/// A real sneak press on a custom mount produces a vehicle-targeted dismount request.
#[test]
fn dismount_press_targets_the_mount_and_retains_a_full_queue_request() {
    let (mut physics, mut ticker) = walked_physics(0);
    let mut locals = super::LocomotionState::default();
    let mut effects = super::LocalMovementEffectTimeline::default();
    let mut speed = super::LocalMovementSpeedAuthority::default();
    let mut frame = speed_frame(Default::default());
    frame.facts.ride = Some(super::RideKind::Other);
    frame.facts.ride_seat = Some([2.0, 3.0, 4.0]);
    frame.facts.mount_runtime_id = Some(u64::MAX);
    frame.sneak.pressed = true;
    frame.sneak.held = true;
    assert!(locals.advance(
        frame,
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1)
    ));
    let position = physics.network_position().unwrap();
    let mut attempts = 0;
    locals.flush_dismount(position, |_| {
        attempts += 1;
        Err(protocol::BatchSendError::Full)
    });
    assert_eq!(
        attempts, 1,
        "a sneak press must ask the vehicle to release its rider"
    );
    let mut accepted = Vec::new();
    locals.flush_dismount(position, |packet| {
        accepted.push(packet);
        Ok(())
    });
    assert_eq!(accepted.len(), 1);
    let bytes = protocol::encode(&accepted[0]).unwrap();
    let bytes = &bytes[2..];
    // Interact: packet id, StopRiding action, full unsigned vehicle id, position presence.
    assert_eq!(&bytes[..2], &[0x21, 3]);
    assert_eq!(
        &bytes[2..12],
        &[255, 255, 255, 255, 255, 255, 255, 255, 255, 1]
    );
    assert_eq!(bytes[12], 1);
    for (axis, value) in position.into_iter().enumerate() {
        let start = 13 + axis * 4;
        assert_eq!(
            f32::from_le_bytes(bytes[start..start + 4].try_into().unwrap()),
            value
        );
    }
    locals.flush_dismount(position, |_| panic!("an accepted press is sent only once"));
    assert_eq!(
        physics.mode(),
        sim::MovementMode::Riding,
        "the server link remains authoritative"
    );
}

/// Losing input, changing mounts or crossing sessions cancels a deferred dismount press.
#[test]
fn dismount_requests_do_not_cross_mount_session_or_focus_boundaries() {
    for boundary in 0..4 {
        let (mut physics, mut ticker) = walked_physics(0);
        let mut locals = super::LocomotionState::default();
        let mut effects = super::LocalMovementEffectTimeline::default();
        let mut speed = super::LocalMovementSpeedAuthority::default();
        for press in [true, false] {
            let mut frame = speed_frame(Default::default());
            frame.facts.ride = Some(super::RideKind::Other);
            frame.facts.mount_runtime_id = Some(20);
            frame.sneak.pressed = press;
            frame.sneak.held = true;
            if !press {
                match boundary {
                    0 => frame.facts.mount_runtime_id = Some(30),
                    1 => frame.active = false,
                    2 => frame.facts.ride = None,
                    _ => ticker.reset(
                        8,
                        ticker.completed_tick(),
                        physics.network_position().unwrap(),
                    ),
                }
            }
            assert!(locals.advance(
                frame,
                &mut physics,
                &mut ticker,
                &mut effects,
                &mut speed,
                &VersionedFloor(1)
            ));
        }
        locals.flush_dismount(physics.network_position().unwrap(), |_| {
            panic!("a stale press must be canceled")
        });
    }
}

/// Boat input exits on jump or sneak; other vehicle contexts keep jump available to the mount.
#[test]
fn dismount_input_follows_the_vehicle_context_without_repeating_held_buttons() {
    for (kind, jump_exits, sneak_exits) in [
        (super::RideKind::Boat, true, true),
        (super::RideKind::Horse, false, true),
        (super::RideKind::Minecart, false, true),
        (super::RideKind::Other, false, true),
    ] {
        for jump_pressed in [false, true] {
            let (mut physics, mut ticker) = walked_physics(0);
            let mut locals = super::LocomotionState::default();
            let mut effects = super::LocalMovementEffectTimeline::default();
            let mut speed = super::LocalMovementSpeedAuthority::default();
            let mut sent = 0;
            for pressed in [true, false] {
                let mut frame = speed_frame(Default::default());
                frame.facts.ride = Some(kind);
                frame.facts.mount_runtime_id = Some(20);
                frame.jump.pressed = pressed && jump_pressed;
                frame.jump.held = jump_pressed;
                frame.sneak.pressed = pressed && !jump_pressed;
                frame.sneak.held = !jump_pressed;
                assert!(locals.advance(
                    frame,
                    &mut physics,
                    &mut ticker,
                    &mut effects,
                    &mut speed,
                    &VersionedFloor(1)
                ));
                locals.flush_dismount(physics.network_position().unwrap(), |_| {
                    sent += 1;
                    Ok(())
                });
            }
            let exits = if jump_pressed { jump_exits } else { sneak_exits };
            assert_eq!(sent, usize::from(exits), "{kind:?} jump {jump_pressed}");
        }
    }
}

/// A press between fixed ticks survives until admission; loading never emits it.
#[test]
fn dismount_press_without_a_tick_survives_backpressure_but_loading_cancels_it() {
    let (mut physics, mut ticker) = walked_physics(0);
    let mut locals = super::LocomotionState::default();
    let mut effects = super::LocalMovementEffectTimeline::default();
    let mut speed = super::LocalMovementSpeedAuthority::default();
    let mut frame = speed_frame(Default::default());
    frame.delta = Duration::ZERO;
    frame.facts.ride = Some(super::RideKind::Other);
    frame.facts.mount_runtime_id = Some(20);
    frame.sneak.pressed = true;
    assert!(locals.advance(
        frame,
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1)
    ));
    let mut sent = 0;
    locals.flush_dismount(physics.network_position().unwrap(), |_| {
        sent += 1;
        Err(protocol::BatchSendError::Full)
    });
    assert_eq!(sent, 1);
    let mut frame = speed_frame(Default::default());
    frame.facts.ride = Some(super::RideKind::Other);
    frame.facts.mount_runtime_id = Some(20);
    frame.sneak.pressed = true;
    frame.hold = Some(super::PhysicsFrameHold {
        registry: sim::CollisionRegistry::new().identity(),
        withhold_input: true,
    });
    assert!(locals.advance(
        frame,
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1)
    ));
    locals.flush_dismount(physics.network_position().unwrap(), |_| {
        panic!("loading invalidates the press")
    });
}

/// A transport pause retains the press even if its held phase ends before physics resumes.
#[test]
fn dismount_press_observed_during_transport_pause_survives_until_recovery() {
    let mut locals = super::LocomotionState::default();
    let mut frame = speed_frame(Default::default());
    frame.facts.ride = Some(super::RideKind::Other);
    frame.facts.mount_runtime_id = Some(20);
    frame.sneak.pressed = true;
    locals.observe_dismount(&frame, 7);
    frame.sneak.pressed = false;
    locals.observe_dismount(&frame, 7);
    let mut requests = 0;
    locals.flush_dismount([0.0; 3], |_| {
        requests += 1;
        Ok(())
    });
    assert_eq!(requests, 1);
}

/// Finalized focus releases cancel deferred taps even after the button is no longer held.
#[test]
fn dismount_requests_cancel_on_finalized_input_authority_release() {
    use semantic_input::{
        Action, DeviceFrame, InputContext, KeyboardMouseFrame, ReleaseReason, SemanticInputRouter,
    };
    for boundary in 0..3 {
        let mut router = SemanticInputRouter::default();
        let mut locals = super::LocomotionState::default();
        let mut frame = speed_frame(Default::default());
        frame.facts.ride = Some(super::RideKind::Other);
        frame.facts.mount_runtime_id = Some(20);
        router
            .route(DeviceFrame {
                keyboard_mouse: Some(KeyboardMouseFrame {
                    activity_sequence: 1,
                    keys: vec![0xe1],
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap();
        let pressed = router.finalize().unwrap();
        frame.sneak = pressed.phases[Action::Sneak as usize];
        assert!(frame.sneak.pressed);
        locals.observe_dismount(&frame, 7);
        locals.flush_dismount([0.0; 3], |_| Err(protocol::BatchSendError::Full));
        router.route(DeviceFrame::default()).unwrap();
        frame.sneak = router.finalize().unwrap().phases[Action::Sneak as usize];
        locals.observe_dismount(&frame, 7);
        match boundary {
            0 => router.set_context(InputContext::UiFocused),
            1 => {}
            _ => router.release_all(ReleaseReason::UiFocusTaken),
        }
        router
            .route(DeviceFrame {
                window_focus_lost: boundary == 1,
                ..Default::default()
            })
            .unwrap();
        let released =
            semantic_input::finalized::FinalizedInput::from_finalized(router.finalize().unwrap());
        frame.active = released.snapshot().is_some();
        let snapshot = released.snapshot().unwrap();
        assert!(
            snapshot.release_reasons.iter().all(Option::is_none),
            "the tap already ended before authority loss"
        );
        frame.input_authority_released = snapshot.authority_released;
        frame.sneak = released.phase(Action::Sneak);
        assert!(
            frame.active,
            "production still publishes a finalized snapshot"
        );
        locals.observe_dismount(&frame, 7);
        locals.flush_dismount([0.0; 3], |_| {
            panic!("authority loss must cancel the deferred tap: {boundary}")
        });
    }
}

/// A pending exit keeps the authoritative seat without turning the rider into a crouch.
#[test]
fn dismount_request_keeps_the_seated_pose_until_the_server_unlinks() {
    let (mut physics, mut ticker) = walked_physics(0);
    let mut locals = super::LocomotionState::default();
    let mut effects = super::LocalMovementEffectTimeline::default();
    let mut speed = super::LocalMovementSpeedAuthority::default();
    let seat = [2.0, 3.0, 4.0];
    for pressed in [true, false] {
        let mut frame = speed_frame(Default::default());
        frame.facts.ride = Some(super::RideKind::Other);
        frame.facts.ride_seat = Some(seat);
        frame.facts.mount_runtime_id = Some(20);
        frame.sneak.pressed = pressed;
        frame.sneak.held = true;
        assert!(locals.advance(
            frame,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1),
        ));
        locals.flush_dismount(physics.network_position().unwrap(), |_| Ok(()));
        assert_eq!(physics.mode(), sim::MovementMode::Riding);
        assert_eq!(physics.state().unwrap().position, sim::Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(physics.latest_sneak_sprint(), Some((false, false)));
        let sample = ticker.pending_samples().last().unwrap().clone();
        assert!(sample.snapshot.flags.bits() & PlayerInputFlags::SNEAK_CURRENT_RAW.bits() != 0);
        assert_eq!(sample.snapshot.flags.bits() & PlayerInputFlags::START_SNEAKING.bits(), 0);
    }
    let mut frame = speed_frame(Default::default());
    frame.sneak.held = true;
    assert!(locals.advance(
        frame,
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1),
    ));
    assert_eq!(physics.mode(), sim::MovementMode::Walking);
    assert_eq!(physics.latest_sneak_sprint(), Some((true, false)));
}

/// Corrections retain physical input while replaying the seated actor's posture.
#[test]
fn mounted_sneak_and_jump_replay_without_crouching_or_moving_the_seat() {
    for kind in [super::RideKind::Boat, super::RideKind::Other] {
        for jumping in [false, true] {
            let (mut physics, _) = walked_physics(1);
            let anchor = super::PhysicsAnchor {
                network_position: physics.network_position().unwrap(),
                tick: physics.state().unwrap().tick,
                on_ground: true,
                velocity: Some([0.0; 3]),
            };
            let seat = [2.0, 3.0, 4.0];
            let frame = physics.advance_with_context(
                Duration::from_millis(100),
                MovementInput {
                    sneaking: true,
                    jumping,
                    ..Default::default()
                },
                PhysicsSampleContext {
                    mode_intent: super::ModeIntent {
                        ride: Some(kind),
                        ride_seat: Some(seat),
                        ..Default::default()
                    },
                    input: super::TickInput {
                        sneak: semantic_input::ActionPhase {
                            held: true,
                            ..Default::default()
                        },
                        sneak_down: true,
                        jump: semantic_input::ActionPhase {
                            held: jumping,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &VersionedFloor(1),
            );
            assert_eq!(frame.samples.len(), 2);
            let eye = physics.render_eye_position();
            for sample in &frame.samples {
                assert_eq!(sample.processed.mode, sim::MovementMode::Riding);
                assert!(!sample.processed.sneaking);
                assert!(sample.input.sneak.held);
            }
            for _ in 0..2 {
                let replay = physics
                    .apply_correction(
                        anchor,
                        PhysicsCorrectionMode::ReplayIfRetained,
                        None,
                        &VersionedFloor(1),
                    )
                    .unwrap();
                assert_eq!(replay.replayed_samples, frame.samples);
                assert_eq!(physics.render_feet_position(), Some(seat));
                assert_eq!(physics.render_eye_position(), eye);
            }
        }
    }
}

/// A streamed mount's rider renders at the seat the mount puts it in, as the mount itself
/// renders, without trailing it by an extra tick.
#[test]
fn a_streamed_seat_renders_where_the_mount_puts_it() {
    let (mut physics, mut ticker) = walked_physics(0);
    let mut locals = super::LocomotionState::default();
    let mut effects = super::LocalMovementEffectTimeline::default();
    let mut speed = super::LocalMovementSpeedAuthority::default();
    for seat in [[2.0, 3.0, 4.0], [2.5, 3.0, 4.0]] {
        let mut frame = speed_frame(Default::default());
        frame.facts.ride = Some(super::RideKind::Other);
        frame.facts.ride_seat = Some(seat);
        frame.facts.mount_runtime_id = Some(20);
        assert!(locals.advance(
            frame,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1),
        ));
    }
    assert_eq!(physics.tick_alpha(), 0.0);
    assert_eq!(physics.render_feet_position(), Some([2.5, 3.0, 4.0]));
}

/// A floor of whole blocks whose top is `y = 1`, each collider attributed to its cell.
struct BlockFloor;

impl CollisionWorld for BlockFloor {
    fn registry_identity(&self) -> CollisionRegistryIdentity {
        fixture_world_identity(1).registry
    }

    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let colliders = self.collision_boxes_with_provenance(query)?;
        Ok(CollisionQuery {
            value: colliders.value.into_iter().map(|collider| collider.aabb).collect(),
            identity: colliders.identity,
        })
    }

    fn collision_boxes_with_provenance(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<Vec<sim::ProvenancedCollider>>, WorldQueryError> {
        let mut value = Vec::new();
        for x in -8..8 {
            for z in -8..8 {
                let min = Vec3::new(f64::from(x), 0.0, f64::from(z));
                let aabb = Aabb::new(min, min + Vec3::ONE);
                if aabb.intersects(query) {
                    value.push(sim::ProvenancedCollider {
                        aabb,
                        block: Some([x, 0, z]),
                        runtime_id: None,
                        cell_has_scaffolding: Some(false),
                    });
                }
            }
        }
        Ok(CollisionQuery {
            value,
            identity: fixture_world_identity(1),
        })
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<sim::BlockPhysicsSample, WorldQueryError> {
        VersionedFloor(1).block_physics(block)
    }
}

/// A frame riding a still boat on the floor, its seat sunk below the floor top like a boat's.
fn boat_frame(server_owns_dismount: bool) -> super::PhysicsFrameInput {
    let mut frame = speed_frame(Default::default());
    frame.movement = [0.0; 2];
    frame.raw_movement = [0.0; 2];
    frame.facts.ride = Some(super::RideKind::Boat);
    frame.facts.ride_seat = Some([0.5, 0.775, 0.5]);
    frame.facts.mount_runtime_id = Some(7);
    frame.facts.mount = Some(super::local_facts::MountFacts {
        position: [0.5, 1.0, 0.5],
        motion: [0.0; 3],
        server_owns_dismount,
    });
    frame
}

/// The network position a dismount request reports.
fn reported_position(packet: &protocol::Packet) -> [f32; 3] {
    let bytes = protocol::encode(packet).unwrap();
    let position = &bytes[bytes.len() - 12..];
    std::array::from_fn(|axis| {
        f32::from_le_bytes(position[axis * 4..axis * 4 + 4].try_into().unwrap())
    })
}

/// Movement state for one rider across frames, and the dismount requests its transport took.
struct Rider {
    physics: LocalPhysicsController,
    ticker: MovementTicker,
    locals: super::LocomotionState,
    effects: super::LocalMovementEffectTimeline,
    speed: super::LocalMovementSpeedAuthority,
    /// How transport answers a dismount request.
    transport: Result<(), protocol::BatchSendError>,
    /// Positions the dismount requests transport took reported.
    sent: Vec<[f32; 3]>,
    /// Attack presses; one waiting keeps the rider seated.
    melee: crate::melee::MeleeRuntime,
    /// Other interaction work the client reports waiting.
    interactions: super::SeatedInteractions,
    /// The client world's mount state: the rider got off on the client and the server's links
    /// for it have not changed since, so movement reads no mount.
    left_on_client: bool,
}

impl Rider {
    fn new() -> Self {
        let (physics, ticker) = walked_physics(0);
        Self {
            physics,
            ticker,
            locals: super::LocomotionState::default(),
            effects: super::LocalMovementEffectTimeline::default(),
            speed: super::LocalMovementSpeedAuthority::default(),
            transport: Ok(()),
            sent: Vec::new(),
            melee: crate::melee::MeleeRuntime::default(),
            interactions: super::SeatedInteractions::default(),
            left_on_client: false,
        }
    }

    /// The server sends a link for the rider, which ends a client-side leave.
    fn server_links(&mut self) {
        self.left_on_client = false;
    }

    /// Runs a frame as the client does: getting off first, then the frame's ticks.
    fn advance(&mut self, frame: super::PhysicsFrameInput) -> bool {
        self.advance_in(frame, &BlockFloor)
    }

    /// Runs a frame in `world`. Returns whether its ticks ran.
    fn advance_in(&mut self, mut frame: super::PhysicsFrameInput, world: &impl CollisionWorld) -> bool {
        if self.left_on_client {
            frame.facts = frame.facts.off_mount();
        }
        // The frame's one tick decision comes first, as the client makes it.
        self.locals.decide_frame_ticks(&self.ticker);
        if self.locals.holds_ticks() {
            self.locals
                .observe_dismount(&frame, self.ticker.session_generation());
            return false;
        }
        let mut interactions = self.interactions;
        interactions.attack_queued |= self.melee.press_pending();
        let transport = self.transport;
        let sent = &mut self.sent;
        let got_off = self.locals.leave_mount(
            &mut frame,
            &mut self.physics,
            &self.ticker,
            world,
            interactions,
            |packet| {
                if transport.is_ok() {
                    sent.push(reported_position(&packet));
                }
                transport
            },
        );
        if got_off {
            self.left_on_client = true;
        }
        assert!(self.locals.advance(
            frame,
            &mut self.physics,
            &mut self.ticker,
            &mut self.effects,
            &mut self.speed,
            world
        ));
        true
    }

    /// Rides a frame on the still boat and sends its movement, then presses sneak on the
    /// next frame, which runs `delta`.
    fn get_off(&mut self, delta: Duration) {
        self.advance(boat_frame(false));
        assert_eq!(self.physics.mode(), sim::MovementMode::Riding);
        self.flush_movement();
        self.advance(sneak_press(delta));
    }

    /// Ticks of the movement packets transport accepts and writes now.
    fn flush_movement(&mut self) -> Vec<u64> {
        let identities = self.send_movement();
        for identity in &identities {
            assert!(self.ticker.acknowledge_physics_send(*identity));
        }
        identities
            .into_iter()
            .map(|identity| identity.tick.get())
            .collect()
    }

    /// Hands the queued movement to transport without its socket writes confirmed yet.
    fn send_movement(&mut self) -> Vec<super::PhysicsSendIdentity> {
        self.send_movement_within(usize::MAX)
    }

    /// Hands at most `budget` queued movement packets to transport, unwritten.
    fn send_movement_within(&mut self, budget: usize) -> Vec<super::PhysicsSendIdentity> {
        let mut identities = Vec::new();
        flush_player_auth_inputs(
            &mut self.ticker,
            budget,
            Some(evidence_context()),
            |identity, _packet| {
                identities.push(identity);
                Ok::<_, &str>(())
            },
        )
        .unwrap();
        identities
    }
}

/// A frame of `delta` on the still boat that presses sneak.
fn sneak_press(delta: Duration) -> super::PhysicsFrameInput {
    let mut press = boat_frame(false);
    press.delta = delta;
    press.sneak.pressed = true;
    press.sneak.held = true;
    press
}

fn close(actual: [f32; 3], expected: [f32; 3]) -> bool {
    (0..3).all(|axis| (actual[axis] - expected[axis]).abs() < 1.0e-4)
}

/// Getting off a boat resting on land stands the rider on the ground beside it and reports
/// that position, rather than leaving it on a seat sunk into the ground.
#[test]
fn getting_off_a_boat_on_land_stands_the_rider_beside_it() {
    let mut rider = Rider::new();
    rider.get_off(speed_frame(Default::default()).delta);
    assert_ne!(rider.physics.mode(), sim::MovementMode::Riding);
    let feet = rider.physics.state().unwrap().position;
    assert!(
        (feet.x - 0.5).abs() < 1.0e-4 && (feet.z + 0.5).abs() < 1.0e-4,
        "a still boat puts its rider on its north side: {feet:?}"
    );
    assert!(
        (1.0..1.01).contains(&feet.y),
        "the rider stands on the ground, not in it: {feet:?}"
    );
    let exit = [0.5, 1.001 + protocol::PLAYER_NETWORK_OFFSET, -0.5];
    assert!(
        rider.sent.len() == 1 && close(rider.sent[0], exit),
        "the request reports where the rider got off: {:?}",
        rider.sent
    );

    // The server's link lags the request; the rider stays off the boat meanwhile.
    rider.advance(boat_frame(false));
    assert_ne!(rider.physics.mode(), sim::MovementMode::Riding);
    assert_eq!(rider.sent.len(), 1, "the request is sent once");
    rider.server_links();
    let mut unlinked = boat_frame(false);
    unlinked.facts = unlinked.facts.off_mount();
    rider.advance(unlinked);
    // Riding again once the server links the rider anew.
    rider.server_links();
    rider.advance(boat_frame(false));
    assert_eq!(rider.physics.mode(), sim::MovementMode::Riding);
}

/// A server that links the rider to the same boat again after it got off seats it again.
#[test]
fn a_resent_link_seats_the_rider_again() {
    let mut rider = Rider::new();
    rider.get_off(speed_frame(Default::default()).delta);
    assert_eq!(rider.sent.len(), 1);
    rider.advance(boat_frame(false));
    assert_ne!(rider.physics.mode(), sim::MovementMode::Riding);
    rider.server_links();
    rider.advance(boat_frame(false));
    assert_eq!(rider.physics.mode(), sim::MovementMode::Riding);
}

/// Seated movement that transport takes no faster than the rider produces it holds no
/// dismount back: while the request waits for it, the frames run no ticks, so the backlog
/// drains and the rider gets off.
#[test]
fn a_seated_movement_backlog_drains_before_the_rider_gets_off() {
    let mut rider = Rider::new();
    for _ in 0..4 {
        rider.advance(boat_frame(false));
    }
    let backlog = rider.ticker.pending_count();
    assert!(backlog >= 4, "{backlog}");
    rider.advance(sneak_press(speed_frame(Default::default()).delta));
    let mut frames = 0;
    while rider.sent.is_empty() && frames <= backlog + 2 {
        assert_eq!(rider.physics.mode(), sim::MovementMode::Riding);
        // Transport takes one packet a frame.
        let ticked = rider.ticker.completed_tick();
        let sent = rider.send_movement_within(1);
        for identity in sent {
            assert!(rider.ticker.acknowledge_physics_send(identity));
        }
        if !rider.advance(boat_frame(false)) {
            assert_eq!(rider.ticker.completed_tick(), ticked, "no seated tick joins it");
        }
        frames += 1;
    }
    assert_eq!(rider.sent.len(), 1, "the rider got off within {frames} frames");
    assert_ne!(rider.physics.mode(), sim::MovementMode::Riding);
}

/// A rider stays seated until transport takes its dismount request, then gets off on a later
/// tick without pressing again.
#[test]
fn a_full_transport_keeps_the_rider_seated_until_it_takes_the_request() {
    let mut rider = Rider::new();
    rider.transport = Err(protocol::BatchSendError::Full);
    rider.get_off(speed_frame(Default::default()).delta);
    assert_eq!(rider.physics.mode(), sim::MovementMode::Riding);
    rider.flush_movement();
    rider.transport = Ok(());
    rider.advance(boat_frame(false));
    assert_ne!(rider.physics.mode(), sim::MovementMode::Riding);
    assert_eq!(rider.sent.len(), 1);
}

/// The dismount request goes out only once the seated movement before it has, so the server
/// sees the seated movement, the request, then movement off the mount.
#[test]
fn seated_movement_still_queued_keeps_the_rider_seated() {
    let mut rider = Rider::new();
    rider.advance(boat_frame(false));
    rider.advance(sneak_press(speed_frame(Default::default()).delta));
    assert_eq!(rider.physics.mode(), sim::MovementMode::Riding);
    assert!(rider.sent.is_empty(), "seated movement goes ahead of the request");
    assert!(!rider.flush_movement().is_empty());
    rider.advance(boat_frame(false));
    assert_ne!(rider.physics.mode(), sim::MovementMode::Riding);
    assert_eq!(rider.sent.len(), 1);
}

/// Vanilla gets the rider off within a tick, so a press in a frame that runs none waits for
/// the next one.
#[test]
fn a_press_between_ticks_gets_off_on_the_next_tick() {
    let mut rider = Rider::new();
    rider.get_off(Duration::ZERO);
    assert_eq!(rider.physics.mode(), sim::MovementMode::Riding);
    assert!(rider.sent.is_empty());
    rider.advance(boat_frame(false));
    assert_ne!(rider.physics.mode(), sim::MovementMode::Riding);
    assert_eq!(rider.sent.len(), 1);
}

/// A mount whose dismounts only the server performs keeps its rider until the server moves it.
#[test]
fn a_server_owned_dismount_waits_for_the_server() {
    let mut rider = Rider::new();
    rider.advance(boat_frame(true));
    rider.flush_movement();
    let mut press = boat_frame(true);
    press.sneak.pressed = true;
    press.sneak.held = true;
    rider.advance(press);
    assert_eq!(rider.physics.mode(), sim::MovementMode::Riding);
    assert!(rider.sent.is_empty(), "the client does not place this rider");
    let seat = rider.physics.network_position().unwrap();
    let mut sent = Vec::new();
    rider.locals.flush_dismount(seat, |packet| {
        sent.push(reported_position(&packet));
        Ok(())
    });
    assert_eq!(sent, [seat]);
}

/// A boat the player steers east puts its rider on its south side, also after the server
/// replaced only the boat's motion, which leaves the predicted pose current.
#[test]
fn a_steered_boat_keeps_its_predicted_heading_through_a_motion_update() {
    let steered = |motion| {
        let mut frame = boat_frame(false);
        frame.facts.vehicle = Some(super::vehicle::SteeredVehicle {
            runtime_id: 7,
            unique_id: -7,
            body: sim::BoatBody {
                width: 1.4,
                height: 0.455,
                height_offset: 0.375,
            },
            position: [0.5, 1.375, 0.5],
            yaw: 0.0,
            velocity: [0.3, 0.0, 0.0],
            seat: [0.0; 3],
            revision: client_world::VehicleRevision { pose: 0, motion },
        });
        frame.facts.vehicle_controlled_locally = true;
        frame
    };
    let mut rider = Rider::new();
    rider.advance(steered(0));
    assert!(rider.physics.predicted_vehicle_feed().is_some());
    rider.flush_movement();
    let seat = rider.physics.state().unwrap().position;
    let mut press = steered(1);
    press.sneak.pressed = true;
    press.sneak.held = true;
    rider.advance(press);
    assert_ne!(rider.physics.mode(), sim::MovementMode::Riding);
    let [x, _, z] = rider.sent[0];
    assert!(
        (f64::from(x) - seat.x).abs() < 1.0e-4 && (f64::from(z) - seat.z - 1.0).abs() < 1.0e-4,
        "an eastbound boat puts its rider south of its seat {seat:?}: {:?}",
        rider.sent
    );
}

/// An attack pressed in the frame the rider would get off reports the seat, so the rider
/// gets off on the next tick: the server sees the attack from the seat, the request, then
/// movement from the exit.
#[test]
fn an_attack_in_the_same_frame_keeps_the_rider_seated_for_a_tick() {
    let mut rider = Rider::new();
    rider.advance(boat_frame(false));
    rider.flush_movement();
    let seat = rider.physics.network_position().unwrap();
    assert!(rider.melee.observe_input(true, false));
    rider.advance(sneak_press(speed_frame(Default::default()).delta));
    assert_eq!(rider.physics.mode(), sim::MovementMode::Riding);
    assert!(rider.sent.is_empty(), "the request waits behind the attack");
    let attack = rider
        .melee
        .press_sample(
            crate::melee::Crosshair::Miss,
            &rider.ticker,
            rider.effects.recent_tick_count(),
            0,
        )
        .unwrap();
    assert!(
        close(attack.position, seat),
        "the attack reports the seat: {attack:?}"
    );
    rider.melee.cancel_press();
    rider.flush_movement();
    rider.advance(boat_frame(false));
    assert_ne!(rider.physics.mode(), sim::MovementMode::Riding);
    let exit = [0.5, 1.001 + protocol::PLAYER_NETWORK_OFFSET, -0.5];
    assert!(
        rider.sent.len() == 1 && close(rider.sent[0], exit),
        "then the rider gets off: {:?}",
        rider.sent
    );
}

/// An item release held behind a seated tick goes out from the seat before the rider gets
/// off.
#[test]
fn a_held_item_release_keeps_the_rider_seated_until_it_goes_out() {
    let mut rider = Rider::new();
    rider.advance(boat_frame(false));
    rider.flush_movement();
    let facing = rider.ticker.completed_tick();
    rider.ticker.hold_release_after_tick(facing, Vec::new());
    rider.advance(sneak_press(speed_frame(Default::default()).delta));
    assert_eq!(rider.physics.mode(), sim::MovementMode::Riding);
    assert!(rider.sent.is_empty(), "the request waits behind the release");
    rider.ticker.confirm_held_release_facing(facing);
    rider.ticker.send_held_release(|_| Ok(()));
    rider.flush_movement();
    rider.advance(boat_frame(false));
    assert_ne!(rider.physics.mode(), sim::MovementMode::Riding);
    assert_eq!(rider.sent.len(), 1);
}

/// A release held behind a tick still to come, as a Riptide release can be, keeps the rider
/// seated without stopping the ticks, so its facing tick is produced and goes out first.
#[test]
fn a_release_facing_a_later_tick_lets_the_ticks_run() {
    let mut rider = Rider::new();
    rider.advance(boat_frame(false));
    rider.flush_movement();
    let facing = rider.ticker.completed_tick() + 1;
    rider.ticker.hold_release_after_tick(facing, Vec::new());
    assert!(
        rider.advance(sneak_press(speed_frame(Default::default()).delta)),
        "the facing tick still runs"
    );
    assert!(rider.ticker.completed_tick() >= facing);
    assert_eq!(rider.physics.mode(), sim::MovementMode::Riding);
    assert!(rider.sent.is_empty(), "the request waits behind the release");
    rider.flush_movement();
    rider.ticker.send_held_release(|_| Ok(()));
    assert!(!rider.ticker.has_held_release());
    rider.flush_movement();
    rider.advance(boat_frame(false));
    assert_ne!(rider.physics.mode(), sim::MovementMode::Riding);
    assert_eq!(rider.sent.len(), 1);
}

/// Seated movement transport took but has not written yet may still be cancelled and sent
/// again, so the rider stays seated until it is written.
#[test]
fn unwritten_seated_movement_keeps_the_rider_seated() {
    let mut rider = Rider::new();
    rider.advance(boat_frame(false));
    let unwritten = rider.send_movement();
    assert!(!unwritten.is_empty());
    rider.advance(sneak_press(speed_frame(Default::default()).delta));
    assert_eq!(rider.physics.mode(), sim::MovementMode::Riding);
    assert!(rider.sent.is_empty(), "the request waits for the seated writes");
    for identity in unwritten {
        assert!(rider.ticker.acknowledge_physics_send(identity));
    }
    rider.flush_movement();
    rider.advance(boat_frame(false));
    assert_ne!(rider.physics.mode(), sim::MovementMode::Riding);
    assert_eq!(rider.sent.len(), 1);
}

/// Each kind of interaction work still waiting from the seat keeps the rider seated until it
/// is done.
#[test]
fn waiting_interaction_work_keeps_the_rider_seated() {
    let waiting = [
        super::SeatedInteractions {
            attack_pressed: true,
            ..Default::default()
        },
        super::SeatedInteractions {
            attack_queued: true,
            ..Default::default()
        },
        super::SeatedInteractions {
            item_release: true,
            ..Default::default()
        },
        super::SeatedInteractions {
            block_use: true,
            ..Default::default()
        },
    ];
    for interactions in waiting {
        let mut rider = Rider::new();
        rider.advance(boat_frame(false));
        rider.flush_movement();
        rider.interactions = interactions;
        rider.advance(sneak_press(speed_frame(Default::default()).delta));
        assert_eq!(
            rider.physics.mode(),
            sim::MovementMode::Riding,
            "{interactions:?}"
        );
        assert!(rider.sent.is_empty(), "{interactions:?}");
        rider.interactions = super::SeatedInteractions::default();
        rider.flush_movement();
        rider.advance(boat_frame(false));
        assert_ne!(rider.physics.mode(), sim::MovementMode::Riding);
        assert_eq!(rider.sent.len(), 1, "{interactions:?}");
    }
}

/// Collision data that is not there yet answers no query.
struct Unloaded;

impl CollisionWorld for Unloaded {
    fn registry_identity(&self) -> CollisionRegistryIdentity {
        fixture_world_identity(1).registry
    }

    fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Err(WorldQueryError::QueryExtentExceeded)
    }

    fn block_physics(&self, _: [i32; 3]) -> Result<sim::BlockPhysicsSample, WorldQueryError> {
        Err(WorldQueryError::QueryExtentExceeded)
    }
}

/// A last tick that waited for collision data keeps the rider seated until a tick completes.
#[test]
fn a_tick_waiting_for_collision_data_keeps_the_rider_seated() {
    let mut rider = Rider::new();
    let mut walking = speed_frame(Default::default());
    walking.movement = [0.0, 1.0];
    rider.advance_in(walking, &Unloaded);
    rider.flush_movement();
    rider.advance(sneak_press(speed_frame(Default::default()).delta));
    assert!(rider.sent.is_empty(), "the request waits for a completed tick");
    rider.flush_movement();
    rider.advance(boat_frame(false));
    assert_ne!(rider.physics.mode(), sim::MovementMode::Riding);
    assert_eq!(rider.sent.len(), 1);
}
