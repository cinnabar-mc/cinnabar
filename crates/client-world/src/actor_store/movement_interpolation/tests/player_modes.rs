use super::*;
use protocol::{MovePlayerEvent, MovePlayerMode};

/// Sends a routed remote player packet with a conspicuous position and body yaw.
fn routed_move(
    store: &mut ActorStore,
    sequence: u64,
    mode: MovePlayerMode,
    pitch: f32,
    head_yaw: f32,
) {
    assert_eq!(
        store.apply(
            1,
            sequence,
            ActorEvent::Move(ActorMoveEvent {
                dimension: 0,
                runtime_id: 1,
                position: [Some(100.0); 3],
                position_origin: ActorPositionOrigin::NetworkOffset,
                pitch: Some(pitch),
                yaw: Some(120.0),
                head_yaw: Some(head_yaw),
                on_ground: Some(true),
                teleported: false,
                force_local_move: false,
                player_mode: Some(mode),
                source_tick: Some(0),
                interpolation: Default::default(),
            }),
        ),
        ActorApplyResult::Updated,
    );
}

#[test]
fn rotation_mode_changes_pitch_without_starting_an_idle_player_lerp() {
    let mut store = player(10.0);
    let before = store.get(1).unwrap().current_pose();
    routed_move(&mut store, 2, MovePlayerMode::Rotation, 345.0, 90.0);
    let actor = store.get(1).unwrap();
    assert_eq!(actor.pitch, -15.0);
    assert_eq!(actor.position, before.position);
    assert_eq!(actor.yaw, before.yaw);
    assert_eq!(actor.head_yaw, before.head_yaw);
    assert_eq!(actor.previous_pose, before);
    assert_eq!(actor.last_received_pose(), before);
    assert_eq!(actor.on_ground, Some(true));
    assert_eq!(actor.interpolation_ticks_remaining, 0);
    store.advance_interpolation_ticks(4);
    let actor = store.get(1).unwrap();
    assert_eq!(actor.pitch, -15.0);
    assert_eq!(actor.position, before.position);
    assert_eq!(actor.yaw, before.yaw);
    assert_eq!(actor.head_yaw, before.head_yaw);
}

#[test]
fn rotation_mode_preserves_an_active_target_and_uses_its_remaining_head_ticks() {
    let mut store = player(0.0);
    store.advance_interpolation_ticks(1);
    let offset = store.get(1).unwrap().network_position_offset();
    store.apply_player_move(
        1,
        2,
        0,
        MovePlayerEvent {
            runtime_id: 1,
            position: [9.0, offset, 0.0],
            pitch: 30.0,
            yaw: 60.0,
            head_yaw: 30.0,
            ..Default::default()
        },
    );
    store.advance_interpolation_ticks(1);
    let target = store.get(1).unwrap().last_received_pose();
    routed_move(&mut store, 3, MovePlayerMode::Rotation, -10.0, 90.0);
    let actor = store.get(1).unwrap();
    assert_eq!(actor.pitch, -10.0);
    assert_eq!(actor.yaw, 20.0);
    assert_eq!(actor.position[0], 3.0);
    assert_eq!(actor.interpolation_ticks_remaining, 2);
    assert_eq!(actor.last_received_pose(), target);
    store.predict_remote_motion(1);
    let predicted = store.pick_pose(1).unwrap();
    store.advance_interpolation_ticks(1);
    let actor = store.get(1).unwrap();
    assert_eq!(actor.current_pose(), predicted);
    assert_eq!(actor.position[0], 6.0);
    assert_eq!(actor.pitch, 10.0);
    assert_eq!(actor.yaw, 40.0);
    assert_eq!(actor.head_yaw, 50.0);
    store.advance_interpolation_ticks(1);
    let actor = store.get(1).unwrap();
    assert_eq!(actor.position[0], 9.0);
    assert_eq!(actor.pitch, 30.0);
    assert_eq!(actor.yaw, 60.0);
    assert_eq!(actor.head_yaw, 90.0);
}

#[test]
fn rotation_mode_keeps_force_completion_and_its_queued_target() {
    let mut store = player(0.0);
    move_pitch(&mut store, 2, 60.0, false, true);
    store.advance_interpolation_ticks(1);
    move_pitch(&mut store, 3, 90.0, false, false);
    routed_move(&mut store, 4, MovePlayerMode::Rotation, 10.0, 90.0);
    // The queued rotation must still take over when the completing target finishes.
    store.advance_interpolation_ticks(8);
    let actor = store.get(1).unwrap();
    assert_eq!(actor.position, [0.0; 3]);
    assert_eq!(actor.pitch, 90.0);
    assert_eq!(actor.yaw, 0.0);
    assert_eq!(actor.interpolation_ticks_remaining, 0);
}

#[test]
fn reset_mode_snaps_on_the_routed_remote_packet_path() {
    let mut store = player(0.0);
    move_pitch(&mut store, 2, 60.0, false, true);
    store.advance_interpolation_ticks(1);
    move_pitch(&mut store, 3, 90.0, false, false);
    routed_move(&mut store, 4, MovePlayerMode::Reset, -10.0, 90.0);
    let actor = store.get(1).unwrap();
    let expected = [100.0, 100.0 - actor.network_position_offset(), 100.0];
    assert_eq!(actor.position, expected);
    assert_eq!(actor.previous_pose.position, expected);
    assert_eq!(actor.interpolation_ticks_remaining, 0);
    assert_eq!(actor.pitch, -10.0);
    assert_eq!(actor.yaw, 120.0);
    assert_eq!(actor.head_yaw, 90.0);
    store.advance_interpolation_ticks(10);
    let actor = store.get(1).unwrap();
    assert_eq!(actor.position, expected);
    assert_eq!(actor.pitch, -10.0);
}
