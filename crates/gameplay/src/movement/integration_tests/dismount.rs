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

/// Movement state for one rider across frames.
struct Rider {
    physics: LocalPhysicsController,
    ticker: MovementTicker,
    locals: super::LocomotionState,
    effects: super::LocalMovementEffectTimeline,
    speed: super::LocalMovementSpeedAuthority,
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
        }
    }

    fn advance(&mut self, frame: super::PhysicsFrameInput) {
        assert!(self.locals.advance(
            frame,
            &mut self.physics,
            &mut self.ticker,
            &mut self.effects,
            &mut self.speed,
            &BlockFloor
        ));
    }

    /// Rides a frame on the still boat, then presses sneak on the next.
    fn get_off(&mut self) {
        self.advance(boat_frame(false));
        assert_eq!(self.physics.mode(), sim::MovementMode::Riding);
        let mut press = boat_frame(false);
        press.sneak.pressed = true;
        press.sneak.held = true;
        self.advance(press);
    }

    /// Ticks of the movement packets transport accepts now.
    fn flush_movement(&mut self) -> Vec<u64> {
        let mut ticks = Vec::new();
        flush_player_auth_inputs(
            &mut self.ticker,
            usize::MAX,
            Some(evidence_context()),
            |identity, _packet| {
                ticks.push(identity.tick.get());
                Ok::<_, &str>(())
            },
        )
        .unwrap();
        ticks
    }
}

/// Getting off a boat resting on land stands the rider on the ground beside it at once and
/// reports that position, rather than leaving it on a seat sunk into the ground.
#[test]
fn getting_off_a_boat_on_land_stands_the_rider_beside_it() {
    let mut rider = Rider::new();
    rider.get_off();
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

    let mut sent = Vec::new();
    let position = rider.physics.network_position().unwrap();
    rider.locals.flush_dismount(position, |packet| {
        sent.push(reported_position(&packet));
        Ok(())
    });
    let [reported] = sent[..] else {
        panic!("one dismount request: {sent:?}")
    };
    assert_eq!([reported[0], reported[2]], [0.5, -0.5]);
    assert!(
        (reported[1] - (1.001 + protocol::PLAYER_NETWORK_OFFSET)).abs() < 1.0e-4,
        "the request reports where the rider got off: {reported:?}"
    );

    // The server's link lags the request; the rider stays off the boat meanwhile.
    rider.advance(boat_frame(false));
    assert_ne!(rider.physics.mode(), sim::MovementMode::Riding);
    let position = rider.physics.network_position().unwrap();
    rider
        .locals
        .flush_dismount(position, |_| panic!("the request is sent once"));
    let mut unlinked = boat_frame(false);
    unlinked.facts.ride = None;
    unlinked.facts.mount_runtime_id = None;
    unlinked.facts.mount = None;
    rider.advance(unlinked);
    // Riding again once the server links the rider anew.
    rider.advance(boat_frame(false));
    assert_eq!(rider.physics.mode(), sim::MovementMode::Riding);
}

/// A server that drops and restores the link between two frames seats the rider again.
#[test]
fn a_restored_link_seats_the_rider_again() {
    let mut rider = Rider::new();
    rider.get_off();
    let position = rider.physics.network_position().unwrap();
    rider.locals.flush_dismount(position, |_| Ok(()));
    rider.advance(boat_frame(false));
    assert_ne!(rider.physics.mode(), sim::MovementMode::Riding);
    let mut relinked = boat_frame(false);
    relinked.facts.mount_changes += 2;
    rider.advance(relinked);
    assert_eq!(rider.physics.mode(), sim::MovementMode::Riding);
}

/// Movement after getting off waits until the dismount request reaches transport, so the
/// server never sees the rider walk while its link still seats it.
#[test]
fn movement_after_getting_off_waits_for_the_dismount_request() {
    let mut rider = Rider::new();
    rider.advance(boat_frame(false));
    let seated = rider.ticker.completed_tick();
    let mut press = boat_frame(false);
    press.sneak.pressed = true;
    press.sneak.held = true;
    rider.advance(press);
    assert!(rider.ticker.completed_tick() > seated);

    let position = rider.physics.network_position().unwrap();
    rider
        .locals
        .flush_dismount(position, |_| Err(protocol::BatchSendError::Full));
    rider.locals.settle_dismount_fence(&mut rider.ticker);
    let before = rider.flush_movement();
    assert!(
        before.iter().all(|tick| *tick <= seated),
        "only seated movement goes ahead of the request: {before:?}"
    );
    assert_eq!(
        rider.ticker.interaction_before_tick(seated + 1),
        None,
        "nor does an interaction before the first tick off the boat"
    );

    rider.locals.flush_dismount(position, |_| Ok(()));
    rider.locals.settle_dismount_fence(&mut rider.ticker);
    let after = rider.flush_movement();
    assert!(
        !after.is_empty() && after.iter().all(|tick| *tick > seated),
        "movement off the boat follows the request: {after:?}"
    );
}

/// Interactions before the next tick report where the player got off, even when the frame
/// it got off in ran no tick.
#[test]
fn interactions_after_getting_off_report_the_exit_before_the_next_tick() {
    let mut rider = Rider::new();
    rider.advance(boat_frame(false));
    let ticked = rider.ticker.completed_tick();
    let mut press = boat_frame(false);
    press.delta = Duration::ZERO;
    press.sneak.pressed = true;
    press.sneak.held = true;
    rider.advance(press);
    assert_eq!(rider.ticker.completed_tick(), ticked);
    let exit = rider.physics.network_position().unwrap();
    assert!((exit[2] + 0.5).abs() < 1.0e-4, "got off north: {exit:?}");
    rider
        .locals
        .flush_dismount(exit, |_| Err(protocol::BatchSendError::Full));
    rider.locals.settle_dismount_fence(&mut rider.ticker);
    rider.flush_movement();
    assert_eq!(
        rider.ticker.between_ticks_sample(),
        None,
        "no interaction reports the exit ahead of the request"
    );
    rider.locals.flush_dismount(exit, |_| Ok(()));
    rider.locals.settle_dismount_fence(&mut rider.ticker);
    rider.flush_movement();
    let close = |position: [f32; 3]| {
        (0..3).all(|axis| (position[axis] - exit[axis]).abs() < 1.0e-4)
    };
    let built = rider.ticker.build_action_state().unwrap().position;
    assert!(close(built), "build actions read the exit: {built:?} != {exit:?}");
    let between = rider.ticker.between_ticks_sample().unwrap().position;
    assert!(close(between), "frame interactions read the exit: {between:?} != {exit:?}");
}

/// A mount whose dismounts only the server performs keeps its rider until the server moves it.
#[test]
fn a_server_owned_dismount_waits_for_the_server() {
    let (mut physics, mut ticker) = walked_physics(0);
    let mut locals = super::LocomotionState::default();
    let mut effects = super::LocalMovementEffectTimeline::default();
    let mut speed = super::LocalMovementSpeedAuthority::default();
    for pressed in [false, true] {
        let mut frame = boat_frame(true);
        frame.sneak.pressed = pressed;
        frame.sneak.held = pressed;
        assert!(locals.advance(
            frame,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &BlockFloor
        ));
    }
    assert_eq!(physics.mode(), sim::MovementMode::Riding);
    let seat = physics.network_position().unwrap();
    let mut sent = Vec::new();
    locals.flush_dismount(seat, |packet| {
        sent.push(reported_position(&packet));
        Ok(())
    });
    assert_eq!(sent, [seat]);
}
