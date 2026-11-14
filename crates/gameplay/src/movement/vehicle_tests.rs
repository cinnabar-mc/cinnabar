use super::*;
use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery,
    SurfaceResponse, WorldQueryError,
};

/// Still water up to y = 63 over a floor at y = 58.
struct Lake;

/// A lake whose terrain beyond x = 100 is unloaded.
struct EdgeLake;

impl CollisionWorld for EdgeLake {
    fn ensure_movement_origin(&self, feet: sim::Vec3) -> Result<(), WorldQueryError> {
        if feet.x >= 100.0 {
            return Err(WorldQueryError::UnloadedChunk(world::ChunkKey {
                dimension: 0,
                x: 6,
                z: 0,
            }));
        }
        Ok(())
    }
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Lake.collision_boxes(query)
    }
    fn primary_is_air(
        &self,
        block: [i32; 3],
    ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
        Lake.primary_is_air(block)
    }
    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Lake.block_physics(block)
    }
}

impl CollisionWorld for Lake {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let mut boxes = Vec::new();
        for x in query.min.x.floor() as i32..=query.max.x.floor() as i32 {
            for y in query.min.y.floor() as i32..=query.max.y.floor().min(58.0) as i32 {
                for z in query.min.z.floor() as i32..=query.max.z.floor() as i32 {
                    let min = sim::Vec3::new(f64::from(x), f64::from(y), f64::from(z));
                    boxes.push(Aabb::new(min, min + sim::Vec3::ONE));
                }
            }
        }
        Ok(CollisionQuery::synthetic(boxes))
    }

    fn primary_is_air(
        &self,
        block: [i32; 3],
    ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
        Ok(Some(CollisionQuery::synthetic(block[1] > 62)))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let water = (59..=62).contains(&block[1]);
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: if water { 8.0 / 9.0 } else { 0.0 },
                flags: if water {
                    BlockPhysicsFlags::WATER
                } else {
                    BlockPhysicsFlags::default()
                },
                surface_response: SurfaceResponse::None,
            }]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

const FORWARD: [f32; 2] = [0.0, 1.0];
/// An oak boat's collision box and origin, as its entity facts give them.
const OAK: sim::BoatBody = sim::BoatBody {
    width: 1.4,
    height: 0.455,
    height_offset: 0.375,
};

fn boat() -> SteeredVehicle {
    SteeredVehicle {
        runtime_id: 7,
        unique_id: -7,
        body: OAK,
        position: [0.5, 62.6, 0.5],
        yaw: 0.0,
        velocity: [0.0; 3],
        seat: [0.0, -0.2, 0.0],
        revision: client_world::VehicleRevision { pose: 0, motion: 0 },
    }
}

/// Steps and commits ticks `first..first + ticks`, returning the last sample.
fn drive(
    prediction: &mut VehiclePrediction,
    vehicle: SteeredVehicle,
    first: u64,
    ticks: u64,
) -> PredictedVehicleSample {
    let mut sample = None;
    for tick in first..first + ticks {
        prediction
            .step(Some(vehicle), FORWARD, &Lake)
            .unwrap()
            .unwrap();
        sample = prediction.commit(tick);
    }
    sample.unwrap()
}

#[test]
fn rowing_moves_the_boat_and_seats_the_rider_on_it() {
    let mut prediction = VehiclePrediction::default();
    let first = drive(&mut prediction, boat(), 1, 1);
    assert_eq!(first.unique_id, -7);
    let sample = drive(&mut prediction, boat(), 2, 20);
    assert!(sample.position[0] > first.position[0] + 1.0);
    assert!(sample.velocity[0] > 0.1);
    assert_eq!(sample.rotation, [0.0, 0.0]);

    let seat = prediction
        .step(Some(boat()), FORWARD, &Lake)
        .unwrap()
        .unwrap();
    let after = prediction.commit(22).unwrap();
    assert_eq!(seat[0], after.position[0]);
    assert!((seat[1] - (after.position[1] - OAK.height_offset - 0.2)).abs() < 1.0e-6);

    let feed = prediction.feed(1.0).unwrap();
    assert_eq!(feed.runtime_id, 7);
    assert_eq!(feed.position[0], after.position[0]);
    assert!((feed.position[1] - (after.position[1] - OAK.height_offset)).abs() < 1.0e-6);
}

#[test]
fn a_server_teleport_restarts_prediction_from_the_new_pose() {
    let mut prediction = VehiclePrediction::default();
    drive(&mut prediction, boat(), 1, 20);
    let teleported = SteeredVehicle {
        position: [40.5, 62.6, 40.5],
        yaw: 90.0,
        revision: client_world::VehicleRevision { pose: 1, motion: 0 },
        ..boat()
    };
    let sample = drive(&mut prediction, teleported, 21, 1);
    assert!((sample.position[0] - 40.5).abs() < 0.1);
    assert!(sample.rotation[1] >= 90.0);
}

#[test]
fn leaving_the_boat_stops_predicting_it() {
    let mut prediction = VehiclePrediction::default();
    drive(&mut prediction, boat(), 1, 3);
    assert_eq!(prediction.step(None, FORWARD, &Lake), Ok(None));
    assert_eq!(prediction.commit(4), None);
    assert_eq!(prediction.feed(1.0), None);
}

#[test]
fn a_boat_on_unloaded_terrain_fails_the_tick_instead_of_seating_the_rider_elsewhere() {
    let mut prediction = VehiclePrediction::default();
    let far = SteeredVehicle {
        position: [150.5, 62.6, 0.5],
        ..boat()
    };
    assert!(prediction.step(Some(far), FORWARD, &EdgeLake).is_err());
    assert_eq!(prediction.commit(4), None);
}

#[test]
fn a_predicted_input_keeps_the_rider_position_and_reports_the_boat_on_the_wire() {
    use crate::test_support::survival_mining::completed;
    let mut ticker = super::super::MovementTicker::default();
    ticker.reset(7, 100, completed(101).position);
    ticker.set_source(super::super::MovementSource::Physics);
    let boat = PredictedVehicleSample {
        unique_id: -7,
        position: [10.0, 62.375, 4.0],
        velocity: [0.3, 0.0, 0.0],
        rotation: [0.0, 90.0],
    };
    let mut sample = completed(101);
    sample.processed.vehicle = Some(boat);
    ticker.enqueue_completed_physics(sample.clone()).unwrap();
    let snapshot = ticker.pending_snapshots()[0];
    // Interactions read the rider; only the wire reports the boat.
    assert_eq!(snapshot.position, sample.position);
    assert_eq!(snapshot.vehicle, Some(boat.input()));
}

#[test]
fn a_correction_continues_prediction_from_the_newest_server_state() {
    let mut predicted = VehiclePrediction::default();
    let at = drive(&mut predicted, boat(), 1, 10);
    let correction = |tick, x: f32| VehicleCorrection {
        runtime_id: 7,
        revision: client_world::VehicleRevision { pose: 0, motion: 0 },
        tick,
        position: [x, at.position[1], at.position[2]],
        velocity: [0.0; 3],
        rotation: [0.0, 45.0],
        angular_velocity: Some(0.0),
        on_ground: false,
    };
    // A correction of a vehicle this session does not predict changes nothing.
    let other = VehicleCorrection {
        runtime_id: 8,
        ..correction(9, 2.0)
    };
    assert_eq!(predicted.correct(&other, &Lake), None);
    // The correction is of tick 9; tick 10 is replayed on it.
    let seat = predicted.correct(&correction(9, 2.0), &Lake).unwrap();
    assert!((seat.seat[0] - 2.0).abs() < 0.5, "{seat:?}");
    let feed = predicted.feed(1.0).unwrap();
    assert!((feed.position[0] - 2.0).abs() < 0.5, "{feed:?}");
    assert!((feed.yaw - 45.0).abs() < 1.0, "{feed:?}");
    // An older correction arriving later does not undo the newer one.
    assert_eq!(predicted.correct(&correction(8, 5.0), &Lake), None);
    assert_eq!(predicted.feed(1.0).unwrap(), feed);
    // Prediction continues from the corrected boat.
    let next = drive(&mut predicted, boat(), 11, 1);
    assert!((next.position[0] - 2.0).abs() < 0.5);
}

/// Steps and commits ticks `first..=last` turning and paddling, returning the committed
/// state after each tick.
fn drive_turning(
    prediction: &mut VehiclePrediction,
    first: u64,
    last: u64,
) -> Vec<(u64, sim::BoatState)> {
    (first..=last)
        .map(|tick| {
            prediction
                .step(Some(boat()), [-1.0, 1.0], &Lake)
                .unwrap()
                .unwrap();
            prediction.commit(tick).unwrap();
            (tick, prediction.session.as_ref().unwrap().state.clone())
        })
        .collect()
}

/// The server reports a tick the client has already predicted past. The corrected boat catches
/// up on the inputs since, so a correction that agrees with the prediction leaves the boat
/// where it was instead of throwing it back to the reported tick.
#[test]
fn a_correction_replays_the_inputs_recorded_since_it() {
    let mut predicted = VehiclePrediction::default();
    let ticks = drive_turning(&mut predicted, 1, 30);
    let (_, reported) = ticks[19].clone();
    let (_, now) = ticks[29].clone();
    assert!(
        (now.yaw - reported.yaw).abs() > 5.0
            && (now.position[0] - reported.position[0]).abs() > 0.5,
        "the boat moves on over ten ticks"
    );
    let correction = VehicleCorrection {
        runtime_id: 7,
        revision: client_world::VehicleRevision { pose: 0, motion: 0 },
        tick: 20,
        position: reported.position,
        velocity: reported.velocity,
        rotation: [0.0, reported.yaw],
        angular_velocity: Some(reported.angular_velocity),
        on_ground: reported.on_ground,
    };
    let seat = predicted.correct(&correction, &Lake).unwrap();
    let feed = predicted.feed(1.0).unwrap();
    for axis in 0..3 {
        assert!(
            (feed.position[axis] + [0.0, OAK.height_offset, 0.0][axis] - now.position[axis]).abs()
                < 1.0e-3,
            "{axis}: {feed:?} {now:?}"
        );
    }
    assert!((feed.yaw - now.yaw).abs() < 1.0e-3, "{feed:?} {now:?}");
    assert!((seat.seat[0] - now.position[0]).abs() < 1.0, "{seat:?}");
    // The rider eases from the seat one tick back, as every ordinary tick does.
    let (_, before_last) = ticks[28].clone();
    let before = BoatState {
        position: before_last.position,
        yaw: before_last.yaw,
        ..now.clone()
    };
    let expected = seat_feet(&before, boat().seat);
    for axis in 0..3 {
        assert!(
            (seat.previous[axis] - expected[axis]).abs() < 1.0e-3,
            "{axis}: {seat:?} {expected:?}"
        );
    }
    // The view eases over the last replayed tick only, not across the whole correction.
    let (_, before_last) = ticks[28].clone();
    let eased = predicted.feed(0.0).unwrap();
    assert!(
        (eased.render_yaw - before_last.yaw).abs() < 1.0e-3,
        "{eased:?}"
    );
}

/// A server that disagrees moves the boat by the disagreement only: the corrected boat plus
/// the replayed ticks, not the corrected boat alone and not the old prediction.
#[test]
fn a_correction_that_disagrees_moves_the_boat_by_the_difference() {
    let mut predicted = VehiclePrediction::default();
    let ticks = drive_turning(&mut predicted, 1, 30);
    let (_, reported) = ticks[19].clone();
    let (_, now) = ticks[29].clone();
    let shifted = [
        reported.position[0] + 2.0,
        reported.position[1],
        reported.position[2],
    ];
    let correction = VehicleCorrection {
        runtime_id: 7,
        revision: client_world::VehicleRevision { pose: 0, motion: 0 },
        tick: 20,
        position: shifted,
        velocity: reported.velocity,
        rotation: [0.0, reported.yaw],
        angular_velocity: Some(reported.angular_velocity),
        on_ground: reported.on_ground,
    };
    predicted.correct(&correction, &Lake).unwrap();
    let feed = predicted.feed(1.0).unwrap();
    let moved = feed.position[0] - now.position[0];
    assert!((moved - 2.0).abs() < 0.5, "{moved}");
    assert!((feed.yaw - now.yaw).abs() < 1.0, "{feed:?} {now:?}");
}

/// A replay the loaded terrain cannot carry out leaves the boat at the corrected state.
#[test]
fn a_correction_whose_replay_cannot_run_keeps_the_corrected_state() {
    let mut predicted = VehiclePrediction::default();
    drive_turning(&mut predicted, 1, 6);
    let correction = VehicleCorrection {
        runtime_id: 7,
        revision: client_world::VehicleRevision { pose: 0, motion: 0 },
        tick: 3,
        position: [150.5, 62.6, 0.5],
        velocity: [0.0; 3],
        rotation: [0.0, 20.0],
        angular_velocity: Some(0.0),
        on_ground: false,
    };
    predicted.correct(&correction, &EdgeLake).unwrap();
    let feed = predicted.feed(1.0).unwrap();
    assert_eq!((feed.position[0], feed.yaw), (150.5, 20.0));
}

/// Inputs from before a new session started are not replayed onto it.
#[test]
fn a_new_session_does_not_replay_the_old_sessions_inputs() {
    let mut predicted = VehiclePrediction::default();
    drive_turning(&mut predicted, 1, 6);
    let teleported = SteeredVehicle {
        position: [40.5, 62.6, 0.5],
        revision: client_world::VehicleRevision { pose: 1, motion: 0 },
        ..boat()
    };
    drive(&mut predicted, teleported, 7, 1);
    assert_eq!(predicted.history.len(), 1);
}

/// A correction arriving after a server change continues the session at that change's
/// revision, so the older change does not replace the corrected boat.
#[test]
fn a_correction_after_a_server_change_continues_at_its_revision() {
    let mut predicted = VehiclePrediction::default();
    let at = drive(&mut predicted, boat(), 1, 10);
    let correction = VehicleCorrection {
        runtime_id: 7,
        revision: client_world::VehicleRevision { pose: 1, motion: 0 },
        tick: 9,
        position: [2.0, at.position[1], at.position[2]],
        velocity: [0.0; 3],
        rotation: [0.0, 0.0],
        angular_velocity: None,
        on_ground: false,
    };
    predicted.correct(&correction, &Lake).unwrap();
    assert_eq!(predicted.feed(1.0).unwrap().revision.pose, 1);
    let changed = SteeredVehicle {
        revision: client_world::VehicleRevision { pose: 1, motion: 0 },
        ..boat()
    };
    let next = drive(&mut predicted, changed, 11, 1);
    assert!((next.position[0] - 2.0).abs() < 0.5);
    // A correction from before that change is ignored.
    let older = VehicleCorrection {
        revision: client_world::VehicleRevision { pose: 0, motion: 0 },
        tick: 12,
        ..correction
    };
    assert_eq!(predicted.correct(&older, &Lake), None);
}

#[test]
fn a_new_session_starts_from_the_boat_s_retained_motion() {
    let mut prediction = VehiclePrediction::default();
    let drifting = SteeredVehicle {
        velocity: [0.3, 0.0, 0.0],
        ..boat()
    };
    prediction.step(Some(drifting), [0.0; 2], &Lake).unwrap();
    let sample = prediction.commit(1).unwrap();
    // Without paddling, the boat keeps gliding instead of starting from a standstill.
    assert!(sample.position[0] > 0.6, "{sample:?}");
    assert!(sample.velocity[0] > 0.0);
}

#[test]
fn a_correction_from_an_earlier_session_is_ignored() {
    let mut predicted = VehiclePrediction::default();
    let at = drive(&mut predicted, boat(), 1, 10);
    let teleported = SteeredVehicle {
        position: [40.5, 62.6, 0.5],
        revision: client_world::VehicleRevision { pose: 1, motion: 0 },
        ..boat()
    };
    drive(&mut predicted, teleported, 11, 2);
    let before = predicted.feed(1.0).unwrap();
    let stale = VehicleCorrection {
        runtime_id: 7,
        revision: client_world::VehicleRevision { pose: 0, motion: 0 },
        tick: 8,
        position: at.position,
        velocity: [0.0; 3],
        rotation: at.rotation,
        angular_velocity: None,
        on_ground: false,
    };
    assert_eq!(predicted.correct(&stale, &Lake), None);
    assert_eq!(predicted.feed(1.0).unwrap(), before);
}

/// The boat renders between its last two ticks on the rider's clock.
#[test]
fn the_fed_boat_renders_between_its_last_two_ticks() {
    let mut prediction = VehiclePrediction::default();
    assert_eq!(prediction.feed(0.5), None);
    for tick in 1..=6 {
        prediction.step(Some(boat()), [-1.0, 1.0], &Lake).unwrap();
        prediction.commit(tick);
    }
    let start = prediction.feed(0.0).unwrap();
    let end = prediction.feed(1.0).unwrap();
    let half = prediction.feed(0.5).unwrap();
    for axis in 0..3 {
        assert!((end.render_position[axis] - end.position[axis]).abs() < 1.0e-4);
    }
    assert!((end.render_yaw - end.yaw).abs() < 1.0e-4);
    assert_ne!(start.render_position, end.render_position);
    for axis in 0..3 {
        let midpoint = (start.render_position[axis] + end.render_position[axis]) / 2.0;
        assert!((half.render_position[axis] - midpoint).abs() < 1.0e-4);
    }
    assert!(start.render_yaw < half.render_yaw && half.render_yaw < end.render_yaw);
    // The latest tick's pose is fed whatever the render point.
    assert_eq!(half.position, end.position);
}

#[test]
fn the_rendered_heading_eases_through_the_current_tick() {
    let mut prediction = VehiclePrediction::default();
    assert_eq!(prediction.render_yaw(0.5), None);
    for tick in 1..=6 {
        prediction.step(Some(boat()), [-1.0, 0.0], &Lake).unwrap();
        prediction.commit(tick);
    }
    let session = prediction.session.as_ref().unwrap();
    let (previous, current) = (session.previous_yaw, session.state.yaw);
    assert!(current > previous);
    assert_eq!(prediction.render_yaw(0.0), Some(previous));
    assert!((prediction.render_yaw(1.0).unwrap() - current).abs() < 1.0e-4);
    let half = prediction.render_yaw(0.5).unwrap();
    assert!(previous < half && half < current);
}

/// Server motion that follows a correction in the same poll keeps the corrected pose and
/// takes the new motion, rather than restarting from the boat's uncorrected pose.
#[test]
fn server_motion_after_a_correction_keeps_the_corrected_pose() {
    let mut predicted = VehiclePrediction::default();
    let at = drive(&mut predicted, boat(), 1, 10);
    let correction = VehicleCorrection {
        runtime_id: 7,
        revision: client_world::VehicleRevision::default(),
        tick: 9,
        position: [2.0, at.position[1], at.position[2]],
        velocity: [0.0; 3],
        rotation: [0.0, 0.0],
        angular_velocity: None,
        on_ground: false,
    };
    predicted.correct(&correction, &Lake).unwrap();
    let pushed = SteeredVehicle {
        revision: client_world::VehicleRevision { pose: 0, motion: 1 },
        velocity: [0.3, 0.0, 0.0],
        ..boat()
    };
    let next = drive(&mut predicted, pushed, 11, 1);
    assert!(next.position[0] > 2.0 && next.position[0] < 2.5, "{next:?}");
}

/// A new session, on boarding or after a server teleport, renders the boat at its first
/// stepped pose, where the rider is seated, instead of easing from the pose before it.
#[test]
fn a_new_session_renders_where_its_rider_is_seated() {
    let pushed = SteeredVehicle {
        velocity: [0.6, 0.0, 0.0],
        ..boat()
    };
    let mut prediction = VehiclePrediction::default();
    drive(&mut prediction, pushed, 1, 1);
    let boarded = prediction.feed(0.0).unwrap();
    assert!(
        boarded.position[0] > pushed.position[0] + 0.1,
        "{boarded:?}"
    );
    assert_eq!(boarded.render_position, boarded.position);
    assert_eq!(boarded.render_yaw, boarded.yaw);

    drive(&mut prediction, pushed, 2, 5);
    let teleported = SteeredVehicle {
        position: [40.5, 62.6, 40.5],
        revision: client_world::VehicleRevision { pose: 1, motion: 0 },
        ..pushed
    };
    drive(&mut prediction, teleported, 7, 1);
    let restarted = prediction.feed(0.0).unwrap();
    assert_eq!(restarted.render_position, restarted.position);
    // A continued session still eases from its last tick.
    drive(&mut prediction, teleported, 8, 1);
    let continued = prediction.feed(0.0).unwrap();
    assert_ne!(continued.render_position, continued.position);
}

/// A correction that disagrees with the prediction changes where the boat is, but not where it
/// is drawn this frame: the drawn pose carries on from where it was and gives way to the
/// corrected one over the next ticks, so the view that follows the boat does not jump or hold.
#[test]
fn a_correction_eases_the_drawn_boat_into_the_corrected_pose() {
    let mut predicted = VehiclePrediction::default();
    let ticks = drive_turning(&mut predicted, 1, 30);
    let (_, latest) = ticks[29].clone();
    let alpha = 0.4;
    let drawn = predicted.feed(alpha).unwrap();
    let correction = VehicleCorrection {
        runtime_id: 7,
        revision: client_world::VehicleRevision { pose: 0, motion: 0 },
        tick: 30,
        position: [
            latest.position[0] + 0.2,
            latest.position[1],
            latest.position[2],
        ],
        velocity: latest.velocity,
        rotation: [0.0, latest.yaw + 4.0],
        angular_velocity: Some(latest.angular_velocity),
        on_ground: latest.on_ground,
    };
    predicted.correct(&correction, &Lake).unwrap();
    let after = predicted.feed(alpha).unwrap();
    assert!((after.yaw - latest.yaw - 4.0).abs() < 1.0e-3, "{after:?}");
    assert!((after.render_yaw - drawn.render_yaw).abs() < 1.0e-3);
    for axis in 0..3 {
        assert!(
            (after.render_position[axis] - drawn.render_position[axis]).abs() < 1.0e-3,
            "{axis}: {after:?} {drawn:?}"
        );
    }
    // The boat keeps turning through the tick after, rather than holding at the corrected yaw.
    drive_turning(&mut predicted, 31, 31);
    let yaw_before = predicted.render_yaw(0.0).unwrap();
    let yaw_after = predicted.render_yaw(1.0).unwrap();
    assert!(
        (yaw_after - yaw_before).abs() > 1.0,
        "{yaw_before} {yaw_after}"
    );
    // The shift is gone a few ticks on, leaving the boat at its own pose.
    drive_turning(&mut predicted, 32, 45);
    let settled = predicted.feed(0.5).unwrap();
    let state = &predicted.session.as_ref().unwrap().state;
    let own = next_yaw_midpoint(predicted.session.as_ref().unwrap());
    assert!(
        (settled.render_yaw - own).abs() < 0.05,
        "{settled:?} {state:?}"
    );
}

/// The yaw midway between a session's previous and latest tick.
fn next_yaw_midpoint(session: &Session) -> f32 {
    let turn = (session.state.yaw - session.previous_yaw + 180.0).rem_euclid(360.0) - 180.0;
    session.previous_yaw + turn * 0.5
}

/// A boat that turns while the server corrects it every few ticks, as it does in play, draws
/// its heading at an even rate: no frame turns much further than an ordinary one, and none
/// holds still.
#[test]
fn the_drawn_heading_stays_even_while_the_server_corrects_a_turning_boat() {
    let mut predicted = VehiclePrediction::default();
    let mut drawn = Vec::new();
    for tick in 1..=120_u64 {
        predicted
            .step(Some(boat()), [-1.0, 1.0], &Lake)
            .unwrap()
            .unwrap();
        predicted.commit(tick).unwrap();
        let frames = [0.0_f32, 0.25, 0.5, 0.75];
        for (index, alpha) in frames.into_iter().enumerate() {
            // The server's view of an earlier tick arrives mid-tick, a few degrees off.
            if tick > 20 && tick % 6 == 0 && index == 2 {
                let reported = tick - 3;
                let state = predicted
                    .history
                    .iter()
                    .find(|input| input.tick == reported)
                    .unwrap()
                    .after
                    .clone();
                let off = if tick % 12 == 0 { 3.0 } else { -3.0 };
                let correction = VehicleCorrection {
                    runtime_id: 7,
                    revision: client_world::VehicleRevision { pose: 0, motion: 0 },
                    tick: reported,
                    position: [
                        state.position[0] + 0.1,
                        state.position[1],
                        state.position[2],
                    ],
                    velocity: state.velocity,
                    rotation: [0.0, state.yaw + off],
                    angular_velocity: Some(state.angular_velocity),
                    on_ground: state.on_ground,
                };
                predicted.correct(&correction, &Lake).unwrap();
            }
            if tick > 20 {
                drawn.push(predicted.render_yaw(alpha).unwrap());
            }
        }
    }
    let steps: Vec<f32> = drawn
        .windows(2)
        .map(|pair| (pair[1] - pair[0] + 180.0).rem_euclid(360.0) - 180.0)
        .collect();
    let mean = steps.iter().sum::<f32>() / steps.len() as f32;
    assert!(mean.abs() > 0.5, "the boat turns: {mean}");
    for (index, step) in steps.iter().enumerate() {
        assert!(
            *step > mean * 0.4 && *step < mean * 2.0,
            "frame {index} turned {step} against a mean of {mean}"
        );
    }
}

/// The rider's eye on a turning boat the server keeps correcting moves at an even pace: the
/// correction snaps the rider to its seat, and neither a jump nor a pause follows.
#[test]
fn the_riders_eye_stays_even_while_the_server_corrects_a_turning_boat() {
    use crate::movement::{ModeIntent, PhysicsSampleContext, RideKind};
    let mut physics = crate::movement::LocalPhysicsController::default();
    physics.reanchor_network_position([0.5, 62.4 + protocol::PLAYER_NETWORK_OFFSET, 0.5], 1, true);
    let context = PhysicsSampleContext {
        mode_intent: ModeIntent {
            ride: Some(RideKind::Boat),
            vehicle: Some(boat()),
            ..ModeIntent::default()
        },
        ..Default::default()
    };
    let input = sim::MovementInput {
        strafe: -1.0,
        forward: 1.0,
        ..sim::MovementInput::default()
    };
    let mut eyes: Vec<[f32; 3]> = Vec::new();
    let mut ticks_seen = 0_u64;
    let mut last_tick = physics.state().map_or(0, |state| state.tick);
    for _ in 0..2400 {
        let frame = physics.advance_with_context(
            std::time::Duration::from_micros(3_125),
            input,
            context.clone(),
            &Lake,
        );
        assert!(frame.blocked.is_none());
        let tick = physics.state().unwrap().tick;
        if tick != last_tick {
            last_tick = tick;
            ticks_seen += 1;
        }
        // Mid-tick, the server reports the latest tick a few degrees and a tenth of a block off.
        if ticks_seen > 20 && ticks_seen % 6 == 0 && physics.tick_alpha() > 0.9 {
            ticks_seen += 1;
            let feed = physics.predicted_vehicle_feed().unwrap();
            let shown = physics.rider_render().unwrap();
            let corrected = physics
                .correct_vehicle(
                    &VehicleCorrection {
                        runtime_id: 7,
                        revision: client_world::VehicleRevision { pose: 0, motion: 0 },
                        tick,
                        position: [
                            feed.position[0] + 0.1,
                            feed.position[1] + OAK.height_offset,
                            feed.position[2],
                        ],
                        velocity: feed.velocity,
                        rotation: [
                            0.0,
                            feed.yaw + if ticks_seen % 12 == 1 { 3.0 } else { -3.0 },
                        ],
                        angular_velocity: None,
                        on_ground: false,
                    },
                    &Lake,
                )
                .unwrap();
            let seat = corrected.seat;
            // A snap lands the rider on the corrected seat at the local tick.
            physics.reanchor_network_position(
                [seat[0], seat[1] + protocol::PLAYER_NETWORK_OFFSET, seat[2]],
                tick,
                false,
            );
            physics.resume_vehicle_render(shown, corrected.previous);
        }
        if ticks_seen > 20 {
            eyes.push(physics.render_eye_position().unwrap());
            // The eye stays on the boat it rides, wherever in a tick the frame falls.
            let boat = physics.predicted_vehicle_feed().unwrap().render_position;
            let eye = physics.render_feet_position().unwrap();
            assert!(
                (eye[0] - boat[0]).abs() < 0.01 && (eye[2] - boat[2]).abs() < 0.01,
                "the eye left the boat: {eye:?} {boat:?}"
            );
        }
    }
    let steps: Vec<f32> = eyes
        .windows(2)
        .map(|pair| (pair[1][0] - pair[0][0]).hypot(pair[1][2] - pair[0][2]))
        .collect();
    let mean = steps.iter().sum::<f32>() / steps.len() as f32;
    assert!(mean > 0.01, "the boat moves: {mean}");
    for (index, step) in steps.iter().enumerate() {
        assert!(
            *step > mean * 0.4 && *step < mean * 2.0,
            "frame {index} moved {step} against a mean of {mean}"
        );
    }
}
