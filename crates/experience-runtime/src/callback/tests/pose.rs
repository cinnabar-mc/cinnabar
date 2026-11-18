//! Server WIT 0.12's actor pose: where the actor faces and how it moves, as snapshotted.

use super::*;

/// The actor's pose is the one the adapter snapshotted, field for field.
#[test]
fn pose_reads_the_snapshot() {
    let pose = PlayerPose {
        yaw: -179.5,
        pitch: 90.0,
        on_ground: false,
        sneaking: true,
        sprinting: false,
        swimming: true,
        gliding: true,
    };
    let mut fixture = Fixture::new();
    fixture.pose = Some(pose);
    let read = fixture.res().pose().unwrap().expect("a pose");
    assert_eq!(
        (read.yaw, read.pitch),
        (-179.5, 90.0),
        "angles are passed through unchanged"
    );
    assert_eq!(
        [
            read.on_ground,
            read.sneaking,
            read.sprinting,
            read.swimming,
            read.gliding
        ],
        [false, true, false, true, true]
    );
}

/// An actor that is not a player has no pose, though the callback has an actor.
#[test]
fn pose_needs_a_snapshotted_player() {
    let mut fixture = Fixture::new();
    fixture.pose = None;
    assert_eq!(
        fixture.res().pose().unwrap().err(),
        Some(WorldError::PlayerUnavailable)
    );
}

/// A pose the adapter could not have sent is malformed: one without an actor, or angles that are
/// not finite or are outside (-180, 180] and [-90, 90].
#[test]
fn malformed_poses_are_refused() {
    let with = |yaw: f64, pitch: f64| {
        let mut fixture = Fixture::new();
        let pose = fixture.pose.as_mut().expect("a pose");
        pose.yaw = yaw;
        pose.pitch = pitch;
        fixture
    };
    for (yaw, pitch) in [(180.0, 90.0), (-179.9, -90.0), (0.0, 0.0)] {
        assert!(with(yaw, pitch).prepared().is_ok(), "{yaw} {pitch}");
    }
    for (yaw, pitch) in [
        (-180.0, 0.0),
        (180.5, 0.0),
        (0.0, -90.5),
        (0.0, 90.5),
        (f64::NAN, 0.0),
        (0.0, f64::INFINITY),
    ] {
        match with(yaw, pitch).prepared() {
            Err(reason) => assert!(reason.contains("pose angles"), "{reason}"),
            Ok(_) => panic!("accepted a pose of {yaw} {pitch}"),
        }
    }
    let no_actor = Fixture {
        actor: None,
        inventory: None,
        ..Fixture::new()
    };
    match no_actor.prepared() {
        Err(reason) => assert!(reason.contains("pose without an actor"), "{reason}"),
        Ok(_) => panic!("accepted a pose without an actor"),
    }
}
