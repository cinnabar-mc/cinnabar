use super::*;
use crate::committed_control::{CommittedGameplayState, ControlObservation};
use client_world::{CommittedControlEvent, ResolvedServerPosition};

/// The gameplay entry points that publish a replayed speed to live movement.
#[derive(Clone, Copy, Debug)]
enum ReplayTrigger {
    Correction,
    Motion,
    Flags,
    Frame,
}

/// Applies one packet to the same owners used by live movement frames.
fn apply_control(
    control: CommittedControlEvent,
    ticker: &mut MovementTicker,
    physics: &mut LocalPhysicsController,
    effects: &mut super::super::LocalMovementEffectTimeline,
    speed: &mut super::super::LocalMovementSpeedAuthority,
) -> Vec<ControlObservation> {
    let mut observations = Vec::new();
    CommittedGameplayState {
        movement: ticker,
        physics,
        effects,
        speed,
        session_generation: 7,
        dimension: 0,
        dimension_transfer_active: false,
    }
    .apply(control, &VersionedFloor(1), |event| {
        observations.push(event)
    });
    observations
}

/// Builds a delayed control that replays the retained ticks after this sample.
fn replay_control(trigger: ReplayTrigger, anchor: &PhysicsMovementSample) -> CommittedControlEvent {
    let mut position = anchor.position;
    position[0] += 0.25;
    match trigger {
        ReplayTrigger::Correction => CommittedControlEvent::PlayerMovementCorrection {
            sequence: 3,
            correction: protocol::PlayerMovementCorrectionEvent {
                position,
                delta: anchor.velocity,
                pitch: 0.0,
                yaw: 0.0,
                subject: protocol::MovementCorrectionSubject::Player,
                on_ground: anchor.grounded_after_tick,
                tick: anchor.tick,
                vehicle_angular_velocity: None,
            },
            resolved: ResolvedServerPosition {
                position,
                surface_anchor: None,
            },
        },
        ReplayTrigger::Motion => CommittedControlEvent::LocalActorMotion {
            sequence: 3,
            event: protocol::ActorMotionEvent {
                actor_runtime_id: 42,
                motion: [0.2, 0.0, 0.0],
                tick: anchor.tick,
            },
        },
        ReplayTrigger::Flags => CommittedControlEvent::LocalMovementFlags {
            sequence: 3,
            tick: anchor.tick,
            flags: flags(|flags| flags.sprinting = Some(false)),
            replacement: false,
            dimensions: None,
        },
        ReplayTrigger::Frame => unreachable!(),
    }
}

/// A delayed correction must preserve speed accepted since the last completed tick.
fn check_pending_speed(update: CommittedControlEvent, expected: f32, trigger: ReplayTrigger) {
    let (mut physics, mut ticker) = walked_physics(0);
    let mut locals = super::super::LocomotionState::default();
    let mut effects = super::super::LocalMovementEffectTimeline::default();
    let mut speed = super::super::LocalMovementSpeedAuthority::default();
    speed.begin_session(7, 0);
    assert!(speed.apply(7, 1, 0, attribute(f64::from(0.1_f32), 0.1, None)));
    for _ in 0..4 {
        assert!(locals.advance(
            speed_frame(Default::default()),
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1),
        ));
    }
    let latest = ticker.completed_tick();
    let anchor = physics.sample_at(latest - 2).unwrap().clone();
    apply_control(update, &mut ticker, &mut physics, &mut effects, &mut speed);
    assert_eq!(speed.current(), Some(f64::from(expected)));

    if matches!(trigger, ReplayTrigger::Frame) {
        reconcile_timeline_rewind(&mut ticker, &mut physics, anchor.tick, &VersionedFloor(1))
            .unwrap();
        let mut frame = speed_frame(Default::default());
        frame.delta = Duration::ZERO;
        assert!(locals.advance(
            frame,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1),
        ));
    } else {
        let control = replay_control(trigger, &anchor);
        let observations =
            apply_control(control, &mut ticker, &mut physics, &mut effects, &mut speed);
        if matches!(trigger, ReplayTrigger::Correction) {
            assert!(observations.iter().any(|event| matches!(
                event,
                ControlObservation::Correction {
                    outcome: PhysicsCorrectionOutcome::Replayed {
                        replayed_ticks: 2,
                        ..
                    },
                    ..
                }
            )));
        }
    }
    assert_eq!(ticker.completed_tick(), latest);
    assert_eq!(speed.current(), Some(f64::from(expected)), "{trigger:?}");

    let mut expected_physics = physics.clone();
    let expected_tick = expected_physics.advance_with_context(
        Duration::from_millis(50),
        MovementInput {
            movement_speed: Some(f64::from(expected)),
            ..physics_movement_input([0.0, 1.0], 180.0, true, false, false, false, None)
        },
        PhysicsSampleContext {
            movement_speed_attribute: speed.attribute(),
            ..Default::default()
        },
        &VersionedFloor(1),
    );
    assert_eq!(expected_tick.completed_ticks, 1);
    assert!(locals.advance(
        speed_frame(Default::default()),
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1),
    ));
    assert_eq!(physics.state(), expected_physics.state());
    assert_eq!(speed.current(), Some(f64::from(expected)));

    // Once recorded, the update must also survive another rewind through its tick.
    reconcile_timeline_rewind(&mut ticker, &mut physics, latest - 1, &VersionedFloor(1)).unwrap();
    let mut frame = speed_frame(Default::default());
    frame.delta = Duration::ZERO;
    assert!(locals.advance(
        frame,
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1),
    ));
    assert_eq!(physics.state(), expected_physics.state());
    assert_eq!(speed.current(), Some(f64::from(expected)));
}

#[test]
fn pending_speed_attribute_survives_correction_and_frame_replay_adoption() {
    for trigger in [
        ReplayTrigger::Correction,
        ReplayTrigger::Motion,
        ReplayTrigger::Flags,
        ReplayTrigger::Frame,
    ] {
        check_pending_speed(
            CommittedControlEvent::LocalMovementSpeed {
                sequence: 2,
                dimension: 0,
                movement: Some(attribute(f64::from(0.2_f32), 0.2, None)),
                movement_received: true,
                underwater: None,
                lava: None,
                air_drag_modifier: None,
                tick: 0,
            },
            0.2,
            trigger,
        );
    }
}

#[test]
fn pending_speed_effect_survives_correction_and_frame_replay_adoption() {
    for trigger in [
        ReplayTrigger::Correction,
        ReplayTrigger::Motion,
        ReplayTrigger::Flags,
        ReplayTrigger::Frame,
    ] {
        check_pending_speed(
            CommittedControlEvent::LocalMovementEffect {
                sequence: 2,
                event: protocol::ActorEffectEvent {
                    dimension: 0,
                    actor_runtime_id: 42,
                    action: protocol::ActorEffectAction::Add,
                    effect_id: protocol_ids::Effect::Speed.wire_value(),
                    amplifier: 1,
                    particles: false,
                    ambient: false,
                    duration_ticks: 600,
                    tick: 104,
                },
            },
            0.1_f32 * 1.4,
            trigger,
        );
    }
}

/// Compares replay before and after the first speed effect reaches a completed tick.
fn check_first_speed_effect_replay(trigger: Option<ReplayTrigger>, metadata_first: bool) {
    let (mut physics, mut ticker) = walked_physics(0);
    let mut locals = super::super::LocomotionState::default();
    let mut effects = super::super::LocalMovementEffectTimeline::default();
    let mut speed = super::super::LocalMovementSpeedAuthority::default();
    speed.begin_session(7, 0);
    for _ in 0..3 {
        assert!(locals.advance(
            speed_frame(Default::default()),
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1),
        ));
    }
    let anchor = ticker.completed_tick();
    let effect = CommittedControlEvent::LocalMovementEffect {
        sequence: if metadata_first { 2 } else { 1 },
        event: protocol::ActorEffectEvent {
            dimension: 0,
            actor_runtime_id: 42,
            action: protocol::ActorEffectAction::Add,
            effect_id: protocol_ids::Effect::Speed.wire_value(),
            amplifier: 0,
            particles: false,
            ambient: false,
            duration_ticks: 600,
            tick: 0,
        },
    };
    let metadata = CommittedControlEvent::LocalMovementFlags {
        sequence: if metadata_first { 1 } else { 2 },
        tick: 0,
        flags: flags(|flags| flags.sprinting = Some(true)),
        replacement: false,
        dimensions: None,
    };
    let controls = if metadata_first {
        [metadata, effect]
    } else {
        [effect, metadata]
    };
    for (index, control) in controls.into_iter().enumerate() {
        apply_control(control, &mut ticker, &mut physics, &mut effects, &mut speed);
        if metadata_first && index == 0 {
            let mut frame = speed_frame(Default::default());
            frame.delta = Duration::ZERO;
            assert!(locals.advance(
                frame,
                &mut physics,
                &mut ticker,
                &mut effects,
                &mut speed,
                &VersionedFloor(1),
            ));
            assert_eq!(ticker.completed_tick(), anchor);
            assert_eq!(speed.current(), None);
        }
    }
    let expected_current = 0.1_f32
        * 1.2
        * if metadata_first {
            sim::SPRINT_SPEED_MULTIPLIER as f32
        } else {
            1.0
        };
    let expected_speed = Some(f64::from(expected_current));
    assert_eq!(speed.current(), expected_speed);
    if let Some(trigger) = trigger {
        let retained = physics.sample_at(anchor - 1).unwrap().clone();
        if matches!(trigger, ReplayTrigger::Frame) {
            reconcile_timeline_rewind(&mut ticker, &mut physics, retained.tick, &VersionedFloor(1))
                .unwrap();
        } else {
            apply_control(
                replay_control(trigger, &retained),
                &mut ticker,
                &mut physics,
                &mut effects,
                &mut speed,
            );
            assert_eq!(speed.current(), expected_speed, "{trigger:?}");
        }
    }
    let mut frame = speed_frame(Default::default());
    frame.delta = Duration::ZERO;
    assert!(locals.advance(
        frame,
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1),
    ));
    assert_eq!(ticker.completed_tick(), anchor);
    assert_eq!(speed.current(), expected_speed, "{trigger:?}");
    assert_eq!(speed.reported_current(), expected_speed, "{trigger:?}");
    assert_eq!(
        speed.prediction_speed(),
        Some(f64::from(
            expected_current / sim::SPRINT_SPEED_MULTIPLIER as f32
        )),
        "{trigger:?}"
    );
    assert!(locals.advance(
        speed_frame(semantic_input::ActionPhase {
            held: true,
            ..Default::default()
        }),
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1),
    ));
    let live_state = physics.state().cloned();
    let live_speed = speed.current();
    assert_eq!(physics.latest_sneak_sprint(), Some((false, true)));
    assert_eq!(
        reconcile_timeline_rewind(&mut ticker, &mut physics, anchor, &VersionedFloor(1)).unwrap(),
        PhysicsCorrectionOutcome::Replayed {
            corrected_tick: anchor,
            replayed_ticks: 1,
        }
    );
    let mut frame = speed_frame(Default::default());
    frame.delta = Duration::ZERO;
    assert!(locals.advance(
        frame,
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1),
    ));
    assert_eq!(physics.state(), live_state.as_ref());
    assert_eq!(speed.current(), live_speed);
    assert_eq!(live_speed, expected_speed);
}

/// A first speed effect followed by live sprint metadata keeps its speed across replay.
#[test]
fn first_speed_effect_before_live_sprint_metadata_matches_replay() {
    for trigger in [
        None,
        Some(ReplayTrigger::Correction),
        Some(ReplayTrigger::Motion),
        Some(ReplayTrigger::Frame),
    ] {
        check_first_speed_effect_replay(trigger, false);
    }
}

/// Live sprint metadata adopted before a first speed effect preserves its modifier in replay.
#[test]
fn live_sprint_metadata_before_first_speed_effect_matches_replay() {
    for trigger in [
        None,
        Some(ReplayTrigger::Correction),
        Some(ReplayTrigger::Motion),
        Some(ReplayTrigger::Frame),
    ] {
        check_first_speed_effect_replay(trigger, true);
    }
}

/// Missing replacement attributes cannot restore speed from the preceding actor's ticks.
#[test]
fn actor_replacement_without_movement_resets_live_and_replayed_speed() {
    for pending_replay in [false, true] {
        let (mut physics, mut ticker) = walked_physics(0);
        let mut locals = super::super::LocomotionState::default();
        let mut effects = super::super::LocalMovementEffectTimeline::default();
        let mut speed = super::super::LocalMovementSpeedAuthority::default();
        speed.begin_session(7, 0);
        assert!(speed.apply(7, 1, 0, attribute(f64::from(0.2_f32), 0.2, None)));
        for _ in 0..3 {
            assert!(locals.advance(
                speed_frame(Default::default()),
                &mut physics,
                &mut ticker,
                &mut effects,
                &mut speed,
                &VersionedFloor(1),
            ));
        }
        let anchor = ticker.completed_tick() - 1;
        assert_eq!(speed.current(), Some(f64::from(0.2_f32)));
        for control in [
            CommittedControlEvent::LocalMovementFlags {
                sequence: 2,
                tick: 0,
                flags: flags(|flags| flags.sprinting = Some(false)),
                replacement: true,
                dimensions: Some(client_world::ExplicitDimensions::for_spawn(2)),
            },
            CommittedControlEvent::LocalMovementSpeed {
                sequence: 2,
                dimension: 0,
                movement: None,
                movement_received: true,
                underwater: None,
                lava: None,
                air_drag_modifier: None,
                tick: 0,
            },
        ] {
            apply_control(control, &mut ticker, &mut physics, &mut effects, &mut speed);
        }
        assert_eq!(speed.current(), None);
        if pending_replay {
            reconcile_timeline_rewind(&mut ticker, &mut physics, anchor, &VersionedFloor(1))
                .unwrap();
            let mut frame = speed_frame(Default::default());
            frame.delta = Duration::ZERO;
            assert!(locals.advance(
                frame,
                &mut physics,
                &mut ticker,
                &mut effects,
                &mut speed,
                &VersionedFloor(1),
            ));
            assert_eq!(speed.current(), None);
        }
        assert!(locals.advance(
            speed_frame(Default::default()),
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1),
        ));
        assert_eq!(speed.current(), None);
        assert_eq!(physics.latest_movement_speed().unwrap().attribute(), None);
        let state = physics.state().cloned();
        for _ in 0..2 {
            reconcile_timeline_rewind(&mut ticker, &mut physics, anchor, &VersionedFloor(1))
                .unwrap();
            let mut frame = speed_frame(Default::default());
            frame.delta = Duration::ZERO;
            assert!(locals.advance(
                frame,
                &mut physics,
                &mut ticker,
                &mut effects,
                &mut speed,
                &VersionedFloor(1),
            ));
            assert_eq!(physics.state(), state.as_ref());
            assert_eq!(speed.current(), None);
            assert_eq!(speed.prediction_speed(), None);
            assert_eq!(speed.reported_current(), None);
        }
    }
}

/// A snap seeds new history from the accepted attribute, which already includes pending effects.
#[test]
fn pending_speed_effect_survives_a_snap_that_clears_history() {
    let (mut physics, mut ticker) = walked_physics(0);
    let mut locals = super::super::LocomotionState::default();
    let mut effects = super::super::LocalMovementEffectTimeline::default();
    let mut speed = super::super::LocalMovementSpeedAuthority::default();
    speed.begin_session(7, 0);
    assert!(speed.apply(7, 1, 0, attribute(f64::from(0.2_f32), 0.2, None)));
    assert!(locals.advance(
        speed_frame(Default::default()),
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1),
    ));
    apply_control(
        CommittedControlEvent::LocalMovementEffect {
            sequence: 2,
            event: protocol::ActorEffectEvent {
                dimension: 0,
                actor_runtime_id: 42,
                action: protocol::ActorEffectAction::Add,
                effect_id: protocol_ids::Effect::Speed.wire_value(),
                amplifier: 0,
                particles: false,
                ambient: false,
                duration_ticks: 600,
                tick: 0,
            },
        },
        &mut ticker,
        &mut physics,
        &mut effects,
        &mut speed,
    );
    let expected = f64::from(0.2_f32 * 1.2);
    assert_eq!(speed.current(), Some(expected));
    let tick = ticker.completed_tick();
    let position = physics.network_position().unwrap();
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        position,
        tick,
        true,
        PhysicsCorrectionMode::Snap,
        &VersionedFloor(1),
    )
    .unwrap();
    assert_eq!(physics.latest_movement_speed(), None);
    for completed in [tick, tick + 1] {
        assert!(locals.advance(
            speed_frame(Default::default()),
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1),
        ));
        assert_eq!(ticker.completed_tick(), completed);
        assert_eq!(speed.current(), Some(expected));
        assert_eq!(speed.reported_current(), Some(expected));
    }
    assert_eq!(
        physics
            .latest_movement_speed()
            .unwrap()
            .attribute()
            .unwrap()
            .current,
        expected
    );
}
