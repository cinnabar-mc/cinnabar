//! Retained block-use picks follow the player's position authority.

use std::time::Duration;

use bevy::prelude::{Quat, Transform, Vec3};
use client_presentation::local_player::{
    InteractionOriginSnapshot, LocalPlayerFrameCarrier, LocalPlayerFrameSample,
};
use gameplay::movement::{
    LocalPhysicsController, flush_player_auth_inputs, reconcile_timeline_rewind,
};
use gameplay::test_support::survival_mining::{completed, evidence, ticker_with_ticks};
use sim::{Aabb, CollisionQuery, CollisionWorld, WorldQueryError};
use simulation_clock::{PhysicsTick, identity::SessionGeneration};

use super::BlockUseRuntime;

const START: [f32; 3] = [0.5, 2.620_01, 0.5];

/// A stone floor under the fixture player.
struct Floor;

impl CollisionWorld for Floor {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(vec![Aabb::new(
            sim::Vec3::new(-16.0, 0.0, -16.0),
            sim::Vec3::new(16.0, 1.0, 16.0),
        )]))
    }
}

/// One published frame's eye ray from `eye`, looking straight ahead.
fn frame_origin(eye: Vec3) -> InteractionOriginSnapshot {
    let mut carrier = LocalPlayerFrameCarrier::default();
    carrier
        .publish(LocalPlayerFrameSample {
            session_generation: SessionGeneration::new(7),
            actor_session_id: 1,
            fifo_sequence: protocol::identity::FifoSequence::new(1),
            physics_tick: PhysicsTick::new(101),
            perspective: semantic_input::PerspectiveMode::FirstPerson,
            world_collision_identity: completed(101).world_identity,
            pose: Transform::from_translation(eye),
            eye,
            feet: eye - Vec3::Y * 1.62,
            rotation: Quat::IDENTITY,
        })
        .unwrap();
    let mut origin = InteractionOriginSnapshot::default();
    origin.publish_from_local_player_frame(&carrier);
    origin
}

/// Knockback replays movement and keeps queued interactions, but the eye ray picked before it
/// starts from where the player no longer is, so a later build action must not reuse it.
#[test]
fn a_replayed_correction_retires_the_retained_pick() {
    let world = Floor;
    let mut ticker = ticker_with_ticks(0);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(START, 100, true);
    let frame = physics.advance(
        Duration::from_millis(100),
        sim::MovementInput::default(),
        &world,
    );
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    let mut written = Vec::new();
    flush_player_auth_inputs(&mut ticker, usize::MAX, Some(evidence()), |identity, _| {
        written.push(identity);
        Ok::<_, ()>(())
    })
    .unwrap();
    for identity in written {
        assert!(ticker.acknowledge_physics_send(identity));
    }

    let mut runtime = BlockUseRuntime::default();
    runtime.retain_pick(
        &frame_origin(Vec3::from_array(START)),
        ticker.position_authority_identity(),
        super::FrameHits {
            endpoint: [0.0; 3],
            ordinary: Some(gameplay::block_use::PickResult::Miss),
            liquid: Some(gameplay::block_use::PickResult::Miss),
        },
    );
    assert!(runtime.pick(&ticker).is_some());

    let interactions = ticker.interaction_authority_identity();
    let tick = physics.state().unwrap().tick - 1;
    let rewind = physics.queue_server_motion([0.4, 0.3, 0.0], tick).unwrap();
    reconcile_timeline_rewind(&mut ticker, &mut physics, rewind, &world).unwrap();
    assert_eq!(ticker.interaction_authority_identity(), interactions);
    assert_eq!(runtime.pick(&ticker), None);
    assert!(runtime.take_picks(&ticker).is_empty());
}

/// A tick consumes the whole FIFO once while retaining the latest frame for an immediate press.
#[test]
fn frame_picks_are_consumed_once_in_arrival_order() {
    let ticker = ticker_with_ticks(0);
    let mut runtime = BlockUseRuntime::default();
    let hits = super::FrameHits {
        endpoint: [0.0; 3],
        ordinary: Some(gameplay::block_use::PickResult::Miss),
        liquid: Some(gameplay::block_use::PickResult::Miss),
    };
    for x in [0.5, 0.6, 0.7] {
        runtime.retain_pick(
            &frame_origin(Vec3::new(x, 2.62, 0.5)),
            ticker.position_authority_identity(),
            hits,
        );
    }
    let picks = runtime.take_picks(&ticker);
    assert_eq!(
        picks.iter().map(|pick| pick.origin.x).collect::<Vec<_>>(),
        [0.5, 0.6, 0.7]
    );
    assert!(runtime.take_picks(&ticker).is_empty());
    assert_eq!(runtime.pick(&ticker), picks.last().copied());
}

/// A later unavailable query does not erase an earlier valid observation in the same batch.
#[test]
fn unavailable_frames_preserve_pending_picks_and_tick_retirement_preserves_only_the_latest() {
    let ticker = ticker_with_ticks(0);
    let mut runtime = BlockUseRuntime::default();
    let hits = super::FrameHits {
        endpoint: [0.0; 3],
        ordinary: Some(gameplay::block_use::PickResult::Miss),
        liquid: Some(gameplay::block_use::PickResult::Miss),
    };
    runtime.retain_pick(
        &frame_origin(Vec3::from_array(START)),
        ticker.position_authority_identity(),
        hits,
    );
    runtime.unavailable_pick();
    assert!(runtime.pick(&ticker).is_none());
    assert_eq!(runtime.take_picks(&ticker).len(), 1);
    runtime.retain_pick(
        &frame_origin(Vec3::from_array(START)),
        ticker.position_authority_identity(),
        hits,
    );
    runtime.discard_pending_picks();
    assert!(runtime.take_picks(&ticker).is_empty());
    assert!(runtime.pick(&ticker).is_some());
}
