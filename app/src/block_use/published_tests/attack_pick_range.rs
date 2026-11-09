//! Frame-time actor picks retain their movement position and pick-range check.

use super::attack_frame_tests::actor_boxes;
use super::attack_hit_position_tests::{attack, unpublished_attack_fixture};
use super::*;
use protocol::test_support::transactions::observe_transaction;

struct EmptyWorld;

impl sim::CollisionWorld for EmptyWorld {
    /// Lets the knockback arc advance without terrain collisions.
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }
}

/// Writes and acknowledges completed movement before a between-tick attack.
fn write_movement(world: &mut World, samples: Vec<gameplay::movement::PhysicsMovementSample>) {
    world.resource_scope(|world, mut movement: Mut<MovementTicker>| {
        for sample in samples {
            movement.enqueue_completed_physics(sample).unwrap();
        }
        let network = world.resource::<NetworkHandle<PackApplication>>();
        let mut written = Vec::new();
        gameplay::movement::flush_player_auth_inputs_guarded(
            &mut movement,
            gameplay::movement::OUTBOX_CAPACITY,
            Some(gameplay::test_support::survival_mining::evidence()),
            |identity, packet, guard, _| {
                network.send_physics_packet(identity, packet, guard)?;
                written.push(identity);
                Ok::<(), client_session::PacketSendError>(())
            },
        )
        .unwrap();
        for identity in written {
            assert!(movement.acknowledge_physics_send(identity));
        }
    });
}

/// Publishes an actor under the interpolated eye after one upward knockback tick.
fn knockback_fixture(
    backwards_motion: f32,
) -> (World, client_session::CapturedPackets, [f32; 3], [f32; 3]) {
    let (mut world, mut captured) = unpublished_attack_fixture(7.5);
    let start = world
        .resource::<MovementTicker>()
        .between_ticks_sample()
        .unwrap()
        .position;
    let mut physics = client_interaction::movement::LocalPhysicsController::default();
    physics.reanchor_network_position(start, 100, false);
    physics.queue_server_motion([0.0, 0.4, backwards_motion], 0);
    let frame = physics.advance(
        Duration::from_millis(60),
        sim::MovementInput::default(),
        &EmptyWorld,
    );
    assert_eq!(frame.completed_ticks, 1);
    assert!(frame.blocked.is_none());
    let current = frame.samples[0].position;
    let eye = physics.render_eye_position().unwrap();
    assert!(eye[1] > start[1] && eye[1] < current[1]);
    assert!((physics.tick_alpha() - 0.2).abs() < 1e-6);
    world.insert_resource(physics);

    {
        let mut movement = world.resource_mut::<MovementTicker>();
        movement.reset(7, 100, start);
        movement.set_source(gameplay::movement::MovementSource::Physics);
    }
    write_movement(&mut world, frame.samples);
    {
        let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        stream.predict_block([4, 2, 6], 0, stream.air_block_id());
    }
    actor_boxes(
        &mut world,
        99,
        &[(
            [eye[0] - 0.3, eye[1] - 0.8, eye[2] - 3.5],
            [eye[0] + 0.3, eye[1] + 0.8, eye[2] - 2.9],
        )],
    );
    let origin = frame_origin(&world, Vec3::from_array(eye), Quat::IDENTITY);
    world.insert_resource(origin);
    let point = super::attack_hit_position_tests::picked_point(&world);
    assert!((gameplay::melee::pick_distance(eye, point) - 2.8).abs() < 1e-5);
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    captured.drain();
    (world, captured, current, point)
}

#[test]
fn attack_pick_range_does_not_retrim_the_published_hit_from_a_later_press_position() {
    let (mut world, mut captured, _, point) = knockback_fixture(0.6);
    let frame = {
        let mut physics =
            world.resource_mut::<client_interaction::movement::LocalPhysicsController>();
        physics.queue_server_motion([0.0, 0.4, 4.0], 0);
        physics.advance(
            Duration::from_millis(50),
            sim::MovementInput::default(),
            &EmptyWorld,
        )
    };
    assert_eq!(frame.completed_ticks, 1);
    let source = frame.samples[0].position;
    let pick_range = gameplay::mining::survival_reach(protocol::PlayerInputMode::Mouse);
    assert!(f64::from(gameplay::melee::pick_distance(source, point)) > pick_range);
    write_movement(&mut world, frame.samples);
    captured.drain();
    attack(&mut world);
    let packets = captured.drain();
    let tx = packets.iter().find_map(observe_transaction).unwrap();
    let actor = tx.actor_use().unwrap();
    assert_eq!(actor.from_position, source);
    assert_eq!(actor.hit_position, point);
}

#[test]
fn attack_pick_range_keeps_a_presented_hit_beyond_three_blocks_from_the_ticked_source() {
    let (mut world, mut captured, source, point) = knockback_fixture(0.6);
    let distance = gameplay::melee::pick_distance(source, point);
    let pick_range = gameplay::mining::survival_reach(protocol::PlayerInputMode::Mouse);
    assert!(distance > 3.0 && f64::from(distance) < pick_range);
    attack(&mut world);
    let packets = captured.drain();
    let tx = packets.iter().find_map(observe_transaction).unwrap();
    let actor = tx.actor_use().unwrap();
    assert_eq!(actor.from_position, source);
    assert_eq!(actor.hit_position, point);
    assert_eq!(world.resource::<MovementTicker>().completed_tick(), 101);
}

#[test]
fn attack_pick_range_trims_a_presented_hit_beyond_the_camera_actors_pick_range() {
    let (mut world, mut captured, source, point) = knockback_fixture(4.0);
    let pick_range = gameplay::mining::survival_reach(protocol::PlayerInputMode::Mouse);
    assert!(f64::from(gameplay::melee::pick_distance(source, point)) > pick_range);
    attack(&mut world);
    let packets = captured.drain();
    assert!(
        packets
            .iter()
            .filter_map(observe_transaction)
            .all(|tx| tx.actor_use().is_none()),
        "the presented actor hit must be trimmed before the attack"
    );
    assert!(matches!(
        world
            .resource::<crate::melee::MeleeTarget>()
            .attack
            .unwrap()
            .crosshair,
        gameplay::melee::Crosshair::Miss
    ));
}
