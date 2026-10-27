//! A Normal-mode local `MovePlayer` eases over vanilla's interpolation steps.

use super::*;
use crate::movement::{PhysicsMovementSample, flush_player_auth_inputs};

/// A floor under the whole fixture; the far half reports unloaded terrain.
struct Floor;

impl CollisionWorld for Floor {
    fn ensure_movement_origin(&self, feet: sim::Vec3) -> Result<(), sim::WorldQueryError> {
        if feet.x >= 64.0 {
            return Err(sim::WorldQueryError::UnloadedChunk(world::ChunkKey {
                dimension: 0,
                x: 4,
                z: 0,
            }));
        }
        Ok(())
    }

    fn collision_boxes(
        &self,
        query: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        let floor = sim::Aabb::new(
            sim::Vec3::new(-128.0, 0.0, -128.0),
            sim::Vec3::new(128.0, 1.0, 128.0),
        );
        Ok(sim::CollisionQuery::synthetic(
            floor
                .intersects(query)
                .then_some(floor)
                .into_iter()
                .collect(),
        ))
    }
}

fn normal_move(position: [f32; 3]) -> CommittedControlEvent {
    CommittedControlEvent::MovePlayer {
        sequence: 3,
        source_cohort: None,
        movement: protocol::MovePlayerEvent {
            runtime_id: 42,
            position,
            yaw: 135.0,
            pitch: -4.0,
            mode: protocol::MovePlayerMode::Normal,
            on_ground: true,
            source_tick: 0,
            ..Default::default()
        },
        resolved: ResolvedServerPosition {
            position,
            surface_anchor: None,
        },
    }
}

fn apply(
    movement: &mut MovementTicker,
    physics: &mut LocalPhysicsController,
    control: CommittedControlEvent,
) -> ControlDisposition {
    let mut effects = LocalMovementEffectTimeline::default();
    effects.begin_session(7);
    let mut speed = LocalMovementSpeedAuthority::default();
    speed.begin_session(7, 0);
    CommittedGameplayState {
        movement,
        physics,
        effects: &mut effects,
        speed: &mut speed,
        session_generation: 7,
        dimension: 0,
        dimension_transfer_active: false,
    }
    .apply(control, &Floor, |_| {})
}

fn advance(physics: &mut LocalPhysicsController, ticks: u32) -> Vec<PhysicsMovementSample> {
    let frame = physics.advance(
        simulation_clock::TICK_DURATION * ticks,
        sim::MovementInput::default(),
        &Floor,
    );
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
    frame.samples
}

fn assert_near(actual: [f32; 3], expected: [f32; 3]) {
    for axis in 0..3 {
        assert!(
            (actual[axis] - expected[axis]).abs() < 1.0e-4,
            "{actual:?} != {expected:?}"
        );
    }
}

/// Encodes each completed tick and records its successful transport acknowledgement.
fn sent_inputs(movement: &mut MovementTicker) -> Vec<protocol::PlayerAuthInputTraceSample> {
    let mut identities = Vec::new();
    let mut inputs = Vec::new();
    flush_player_auth_inputs(
        movement,
        usize::MAX,
        Some(crate::test_support::survival_mining::evidence()),
        |identity, packet| {
            identities.push(identity);
            inputs.push(protocol::player_auth_input_trace_sample(&packet).unwrap());
            Ok::<_, ()>(())
        },
    )
    .unwrap();
    for identity in identities {
        assert!(movement.acknowledge_physics_send(identity));
    }
    inputs
}

/// Each sent tick closes 1/3, then 1/2 of the remaining gap and the third lands on the
/// target, with no correction reset and no teleport acknowledgement.
#[test]
fn a_normal_move_player_eases_the_sent_positions_over_three_ticks() {
    let (mut movement, mut physics, _, _) = owners();
    let start = physics.network_position().unwrap();
    let target = [12.5, start[1], 6.5];
    let disposition = apply(&mut movement, &mut physics, normal_move(target));
    assert_eq!(disposition, ControlDisposition::Aim);
    assert_eq!(physics.network_position(), Some(start));

    let samples = advance(&mut physics, 4);
    let positions: Vec<_> = samples.iter().map(|sample| sample.position).collect();
    assert_near(positions[0], [4.5, start[1], 2.5]);
    assert_near(positions[1], [8.5, start[1], 4.5]);
    assert_eq!(positions[2], target);
    assert_eq!(positions[3], target);
    assert_eq!(
        samples.iter().map(|sample| sample.tick).collect::<Vec<_>>(),
        [101, 102, 103, 104]
    );

    for sample in samples {
        let position = sample.position;
        let velocity = sample.velocity;
        movement.enqueue_completed_physics(sample).unwrap();
        let sent = movement.newest_unsent_sample().unwrap();
        assert_eq!(sent.position, position);
        assert_eq!(sent.delta, velocity);
    }
    let sent = sent_inputs(&mut movement);
    assert_eq!(
        sent.iter().map(|input| input.tick).collect::<Vec<_>>(),
        [101, 102, 103, 104]
    );
    assert_eq!(
        sent.iter().map(|input| input.position).collect::<Vec<_>>(),
        positions
    );
    assert!(
        sent.iter()
            .all(|input| !input.flag_names.contains(&"HandledTeleport"))
    );
}

/// Rendering eases from where each tick started rather than jumping a third at a time.
#[test]
fn the_rendered_eye_slides_between_eased_ticks() {
    let (mut movement, mut physics, _, _) = owners();
    let start = physics.network_position().unwrap();
    apply(
        &mut movement,
        &mut physics,
        normal_move([12.5, start[1], 6.5]),
    );
    advance(&mut physics, 1);
    let half = physics.advance(
        simulation_clock::TICK_DURATION / 2,
        sim::MovementInput::default(),
        &Floor,
    );
    assert!(half.samples.is_empty());
    let x = physics.render_feet_position().unwrap()[0];
    assert!(x > 0.5 && x < 4.5, "{x}");
}

/// A correction replaying across eased ticks steps the ease again from the corrected state.
#[test]
fn a_correction_replay_reproduces_the_ease() {
    let (mut movement, mut physics, _, _) = owners();
    let start = physics.network_position().unwrap();
    let mut samples = advance(&mut physics, 1);
    let target = [12.5, start[1], 6.5];
    apply(&mut movement, &mut physics, normal_move(target));
    samples.extend(advance(&mut physics, 2));
    for sample in samples {
        movement.enqueue_completed_physics(sample).unwrap();
    }
    let corrected = [3.5, start[1], 0.5];
    crate::movement::reconcile_physics_anchor(
        &mut movement,
        &mut physics,
        crate::movement::PhysicsAnchor {
            network_position: corrected,
            tick: 101,
            on_ground: true,
            velocity: Some([0.0; 3]),
        },
        PhysicsCorrectionMode::ReplayIfRetained,
        &Floor,
    )
    .unwrap();
    // Two of three steps from the corrected anchor, then the last step lands.
    assert_near(
        physics.network_position().unwrap(),
        [3.5 + 9.0 * (2.0 / 3.0), start[1], 0.5 + 6.0 * (2.0 / 3.0)],
    );
    advance(&mut physics, 1);
    assert_eq!(physics.network_position(), Some(target));
}

/// A teleport snap ends an ease in flight, as vanilla's teleport clears the interpolator.
#[test]
fn a_teleport_snap_cancels_the_ease() {
    let (mut movement, mut physics, _, _) = owners();
    let start = physics.network_position().unwrap();
    apply(
        &mut movement,
        &mut physics,
        normal_move([12.5, start[1], 6.5]),
    );
    advance(&mut physics, 1);
    let teleport = [-20.5, start[1], -20.5];
    physics.reanchor_network_position(teleport, 101, true);
    advance(&mut physics, 3);
    assert_eq!(physics.network_position(), Some(teleport));
}

/// An unloaded target keeps the correction path, which places it directly.
#[test]
fn an_unloaded_target_does_not_ease() {
    let (mut movement, mut physics, _, _) = owners();
    let start = physics.network_position().unwrap();
    let target = [80.5, start[1], 0.5];
    let disposition = apply(&mut movement, &mut physics, normal_move(target));
    assert_eq!(
        disposition,
        ControlDisposition::Spatial(SpatialReset::Correction)
    );
    assert_eq!(physics.network_position(), Some(target));
}

/// A raised landing is followed by delayed echoes of each eased position, then falling.
#[test]
fn raised_landing_corrections_do_not_pull_the_player_back() {
    let (mut movement, mut physics, _, _) = owners();
    let start = physics.network_position().unwrap();
    let target = [14.75, start[1] + 0.875, -17.5];
    // Retain a pre-landing tick so delayed corrections have a replay anchor.
    for sample in advance(&mut physics, 1) {
        movement.enqueue_completed_physics(sample).unwrap();
    }
    sent_inputs(&mut movement);
    apply(&mut movement, &mut physics, normal_move(target));
    assert!(physics.state().unwrap().on_ground);
    let expected = [
        [5.25, 2.911_676_6, -5.5],
        [10.0, 3.203_343_4, -11.5],
        target,
    ];
    let mut landing_samples: Vec<PhysicsMovementSample> = Vec::new();
    for step in 0..6 {
        // Each echo arrives two ticks later, including one after easing finishes.
        if step >= 2 {
            let corrected = &landing_samples[step - 2];
            let before = physics.network_position().unwrap();
            crate::movement::reconcile_prediction_correction(
                &mut movement,
                &mut physics,
                corrected.position,
                corrected.tick,
                false,
                corrected.velocity,
                &Floor,
            )
            .unwrap();
            assert_eq!(physics.network_position(), Some(before));
        }
        let sample = advance(&mut physics, 1).pop().unwrap();
        if step < expected.len() {
            assert_near(sample.position, expected[step]);
            assert_near(sample.velocity, [0.0, -0.0784, 0.0]);
        } else {
            assert_eq!(sample.position[0], target[0]);
            assert_eq!(sample.position[2], target[2]);
            assert!(sample.position[1] < target[1]);
            assert!(sample.velocity[1] < landing_samples[step - 1].velocity[1]);
        }
        movement.enqueue_completed_physics(sample.clone()).unwrap();
        let sent = sent_inputs(&mut movement);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].position, sample.position);
        assert_eq!(sent[0].pos_delta, sample.velocity);
        assert!(!sent[0].flag_names.contains(&"HandledTeleport"));
        landing_samples.push(sample);
    }
}

/// Stamped motion arriving with a landing is cleared by interpolation in live ticks and replay.
#[test]
fn an_eased_tick_clears_stamped_motion_before_travel() {
    let (mut movement, mut physics, _, _) = owners();
    let start = physics.network_position().unwrap();
    for sample in advance(&mut physics, 1) {
        movement.enqueue_completed_physics(sample).unwrap();
    }
    apply(
        &mut movement,
        &mut physics,
        normal_move([12.5, start[1], 6.5]),
    );
    physics.queue_server_motion([1.0, 0.5, -0.5], 101);
    let sample = advance(&mut physics, 1).pop().unwrap();
    assert_near(sample.position, [4.5, start[1], 2.5]);
    assert_near(sample.velocity, [0.0, -0.0784, 0.0]);
    movement.enqueue_completed_physics(sample).unwrap();
    crate::movement::reconcile_physics_anchor(
        &mut movement,
        &mut physics,
        crate::movement::PhysicsAnchor {
            network_position: start,
            tick: 101,
            on_ground: true,
            velocity: Some([0.0; 3]),
        },
        PhysicsCorrectionMode::ReplayIfRetained,
        &Floor,
    )
    .unwrap();
    assert_near(physics.network_position().unwrap(), [4.5, start[1], 2.5]);
}

/// Float rounding belongs to the sent position, before converting back to collision feet.
#[test]
fn an_eased_height_uses_network_position_precision() {
    let (mut movement, mut physics, _, _) = owners();
    let start = physics.network_position().unwrap();
    let target = [12.5, start[1] + 0.875, 6.5];
    apply(&mut movement, &mut physics, normal_move(target));
    let sample = advance(&mut physics, 1).pop().unwrap();
    assert_eq!(sample.position[1], 2.911_676_6);
}

/// A position update followed by a marked landing still snaps and acknowledges once.
#[test]
fn an_actor_position_then_teleport_cancels_a_normal_landing() {
    let (mut movement, mut physics, _, _) = owners();
    let start = physics.network_position().unwrap();
    apply(
        &mut movement,
        &mut physics,
        normal_move([12.5, start[1], 6.5]),
    );
    for sample in advance(&mut physics, 1) {
        movement.enqueue_completed_physics(sample).unwrap();
    }
    sent_inputs(&mut movement);
    let target = [-20.5, start[1], -20.5];
    use protocol::test_support::packets::{ActorAbsoluteFixture, move_actor_absolute_event};
    let protocol::WorldEvent::Actor(protocol::ActorEvent::Move(actor_move)) =
        move_actor_absolute_event(ActorAbsoluteFixture {
            runtime_id: 42,
            flags: 1,
            position: target,
            ..Default::default()
        })
        .unwrap()
        .unwrap()
    else {
        panic!("absolute actor movement");
    };
    apply(
        &mut movement,
        &mut physics,
        CommittedControlEvent::LocalActorMove {
            sequence: 4,
            movement: actor_move,
            resolved: ResolvedServerPosition {
                position: target,
                surface_anchor: None,
            },
        },
    );
    let mut teleport = normal_move(target);
    if let CommittedControlEvent::MovePlayer { movement, .. } = &mut teleport {
        movement.mode = protocol::MovePlayerMode::Teleport;
        movement.teleported = true;
    }
    apply(&mut movement, &mut physics, teleport);
    assert_eq!(physics.network_position(), Some(target));
    assert!(advance(&mut physics, 1).is_empty());
    let samples = advance(&mut physics, 3);
    assert_eq!(samples.len(), 3);
    assert!(samples.iter().all(|sample| sample.position == target));
    for sample in samples {
        movement.enqueue_completed_physics(sample).unwrap();
    }
    let sent = sent_inputs(&mut movement);
    assert_eq!(
        sent.iter()
            .filter(|input| input.flag_names.contains(&"HandledTeleport"))
            .count(),
        1
    );
    assert_eq!(movement.pending_teleport_ack_admitted_ticks(), None);
}
