use super::*;
use crate::movement::{PhysicsAnchor, PhysicsSampleContext};

/// Loaded empty terrain allows exact height assertions without a landing collision.
struct Air;

impl CollisionWorld for Air {
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }
}

/// The adjacent column is unavailable, while collision probes remain inside the loaded one.
struct ColumnEdge;

impl CollisionWorld for ColumnEdge {
    fn ensure_movement_origin(&self, feet: sim::Vec3) -> Result<(), sim::WorldQueryError> {
        if feet.x >= 16.0 {
            Err(sim::WorldQueryError::UnloadedChunk(world::ChunkKey {
                dimension: 0,
                x: 1,
                z: 0,
            }))
        } else {
            Ok(())
        }
    }

    fn collision_boxes(
        &self,
        query: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Floor.collision_boxes(query)
    }
}

/// Eased aim drives both travel and every completed packet, including catch-up ticks.
#[test]
fn rotation_eases_the_travel_direction_and_sent_aim() {
    let (mut movement, mut physics, _, _) = owners();
    let start = physics.network_position().unwrap();
    let anchor = advance(&mut physics, 1).remove(0);
    movement.enqueue_completed_physics(anchor.clone()).unwrap();
    apply(&mut movement, &mut physics, normal_move(start));
    physics.set_server_movement_lerp_rotation([90.0, 30.0]);
    let frame = physics.advance_with_context(
        simulation_clock::TICK_DURATION * 3,
        sim::MovementInput {
            forward: 1.0,
            ..Default::default()
        },
        PhysicsSampleContext::default(),
        &Floor,
    );
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
    assert_eq!(frame.samples.len(), 3);
    for (sample, yaw) in frame.samples.into_iter().zip([30.0_f32, 60.0, 90.0]) {
        assert_eq!(sample.yaw, yaw);
        assert_eq!(sample.pitch, yaw / 3.0);
        assert_eq!(sample.head_yaw, yaw);
        let radians = yaw.to_radians();
        assert!(sample.movement[0] < 0.0);
        let projected = sample.movement[0] * radians.cos() + sample.movement[2] * radians.sin();
        assert!(projected.abs() < 1.0e-5, "{sample:?}");
        let pitch = sample.pitch.to_radians();
        assert_near(
            sample.camera_orientation,
            [
                -radians.sin() * pitch.cos(),
                -pitch.sin(),
                radians.cos() * pitch.cos(),
            ],
        );
        movement.enqueue_completed_physics(sample).unwrap();
    }
    crate::movement::reconcile_physics_anchor(
        &mut movement,
        &mut physics,
        PhysicsAnchor {
            network_position: anchor.position,
            tick: anchor.tick,
            on_ground: true,
            velocity: Some([0.0; 3]),
        },
        PhysicsCorrectionMode::ReplayIfRetained,
        &Floor,
    )
    .unwrap();
    let sent = sent_inputs(&mut movement);
    assert_eq!(
        sent.iter().map(|sample| sample.yaw).collect::<Vec<_>>(),
        [30.0, 60.0, 90.0]
    );
    assert_eq!(
        sent.iter().map(|sample| sample.pitch).collect::<Vec<_>>(),
        [10.0, 20.0, 30.0]
    );
    assert_eq!(physics.take_eased_rotation(), Some([90.0, 30.0]));
}

/// Turning the mouse between steps changes the remaining gap; yaw crosses the short arc.
#[test]
fn rotation_easing_uses_the_current_aim_and_shortest_arc() {
    let (mut movement, mut physics, _, _) = owners();
    let start = physics.network_position().unwrap();
    apply(&mut movement, &mut physics, normal_move(start));
    physics.set_server_movement_lerp_rotation([-170.0, 0.0]);
    for (current, expected) in [
        (170.0, 176.666_67),
        (-173.333_33, -171.666_66),
        (-160.0, -170.0),
    ] {
        let frame = physics.advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput {
                yaw_degrees: current,
                ..Default::default()
            },
            &Floor,
        );
        assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
        assert!((frame.samples[0].yaw - expected).abs() < 0.0001);
    }
}

/// The committed vehicle resolution selects the movement path independently of its ID.
#[test]
fn committed_vehicle_resolution_selects_the_movement_path() {
    for resolved in [false, true] {
        let (mut movement, mut physics, _, _) = owners();
        let start = physics.network_position().unwrap();
        let mut control = normal_move([12.5, start[1], 6.5]);
        if let CommittedControlEvent::MovePlayer {
            movement,
            riding_actor_resolved,
            ..
        } = &mut control
        {
            movement.riding_runtime_id = 77;
            *riding_actor_resolved = resolved;
        }
        let disposition = apply(&mut movement, &mut physics, control);
        if resolved {
            assert_eq!(
                disposition,
                ControlDisposition::Spatial(SpatialReset::Correction)
            );
        } else {
            assert_eq!(disposition, ControlDisposition::Aim);
            assert_eq!(physics.network_position(), Some(start));
            assert_near(advance(&mut physics, 1)[0].position, [4.5, start[1], 2.5]);
        }
    }
}

/// An unavailable adjacent target places directly, then the previous ease resumes unchanged.
#[test]
fn an_unloaded_target_preserves_the_active_ease_and_sent_positions() {
    let (mut movement, mut physics, _, _) = owners();
    let start = physics.network_position().unwrap();
    let target = [14.5, start[1], 0.5];
    apply_in(
        &mut movement,
        &mut physics,
        normal_move(target),
        &ColumnEdge,
    );
    movement
        .enqueue_completed_physics(advance(&mut physics, 1).remove(0))
        .unwrap();
    let unloaded = [16.5, start[1], 0.5];
    let disposition = apply_in(
        &mut movement,
        &mut physics,
        normal_move(unloaded),
        &ColumnEdge,
    );
    assert_eq!(
        disposition,
        ControlDisposition::Spatial(SpatialReset::Correction)
    );
    assert_eq!(physics.network_position(), Some(unloaded));
    let frame = physics.advance(
        simulation_clock::TICK_DURATION * 2,
        sim::MovementInput::default(),
        &ColumnEdge,
    );
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
    assert_eq!(frame.samples.len(), 2);
    assert_eq!(frame.samples[0].position, [15.5, start[1], 0.5]);
    assert_eq!(frame.samples[1].position, target);
    for sample in frame.samples {
        movement.enqueue_completed_physics(sample).unwrap();
    }
    let sent = sent_inputs(&mut movement);
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[1].position, [15.5, start[1], 0.5]);
    assert_eq!(sent[2].position, target);
}

/// Every eased height retains its native bits through sampling, replay and packet encoding.
#[test]
fn eased_packet_height_is_exact_in_live_replayed_and_sent_positions() {
    let (mut movement, mut physics, _, _) = owners();
    let anchor = physics
        .advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &Air,
        )
        .samples
        .remove(0);
    movement.enqueue_completed_physics(anchor.clone()).unwrap();
    let target = [12.5, 0.1_f32, 6.5];
    apply_in(&mut movement, &mut physics, normal_move(target), &Air);
    let frame = physics.advance(
        simulation_clock::TICK_DURATION * 3,
        sim::MovementInput::default(),
        &Air,
    );
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
    assert_eq!(frame.samples.len(), 3);
    let mut height = anchor.position[1];
    let expected: Vec<_> = frame
        .samples
        .iter()
        .enumerate()
        .map(|(step, _)| {
            height = if step == 2 {
                target[1]
            } else {
                height + (target[1] - height) * (1.0 / (3 - step) as f32)
            };
            height.to_bits()
        })
        .collect();
    assert_eq!(
        frame
            .samples
            .iter()
            .map(|sample| sample.position[1].to_bits())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(physics.network_position(), Some(target));
    for sample in frame.samples {
        movement.enqueue_completed_physics(sample).unwrap();
    }
    crate::movement::reconcile_physics_anchor(
        &mut movement,
        &mut physics,
        PhysicsAnchor {
            network_position: anchor.position,
            tick: anchor.tick,
            on_ground: true,
            velocity: Some([0.0; 3]),
        },
        PhysicsCorrectionMode::ReplayIfRetained,
        &Air,
    )
    .unwrap();
    assert_eq!(physics.network_position(), Some(target));
    for (tick, bits) in (102..=104).zip(&expected) {
        assert_eq!(
            physics.sample_at(tick).unwrap().position[1].to_bits(),
            *bits
        );
    }
    let sent = sent_inputs(&mut movement);
    assert_eq!(
        sent.iter()
            .map(|sample| sample.position[1].to_bits())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(sent.last().unwrap().position, target);
}
