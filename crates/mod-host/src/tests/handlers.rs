use super::*;

/// Loads a fixture after substituting each `(placeholder, code)` pair.
fn load(
    fixture: &str,
    substitutions: &[(&str, &str)],
    grants: ModGrants,
) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("handler.wat");
    let source = substitutions
        .iter()
        .fold(fixture.to_owned(), |source, (from, to)| {
            source.replace(from, to)
        });
    std::fs::write(&path, source).unwrap();
    let host = ModHost::load_with_grants(&path, grants).unwrap();
    (directory, host)
}

/// A sound handler whose body runs first and that returns `cancelled` as computed.
fn sound_handler(body: &str, cancelled: &str, audio: bool) -> (tempfile::TempDir, ModHost) {
    load(
        include_str!("sound.wat"),
        &[
            ("$BODY", body),
            ("$LOOPING", "local.get 6"),
            ("$CANCELLED", cancelled),
        ],
        ModGrants {
            audio,
            ..Default::default()
        },
    )
}

/// One positional, non-looping request with the given incoming cancellation.
fn sound(cancelled: bool) -> SoundContext {
    SoundContext {
        name: "fixture.sound".into(),
        position: Some(SoundPosition {
            x: 1.,
            y: 64.,
            z: 0.,
        }),
        looping: false,
        cancelled,
    }
}

#[test]
fn sound_handler_requires_explicit_export_and_grant() {
    let (_dir, mut host) = sound_handler("", "i32.const 1", false);
    assert!(!host.has_sound_handler());
    assert!(host.handle_sound(sound(false)).is_err());
    assert!(host.is_active());
}

#[test]
fn sound_handler_cancels_and_cancellation_is_sticky() {
    let (_dir, mut cancel) = sound_handler("", "i32.const 1", true);
    assert!(cancel.has_sound_handler());
    assert_eq!(cancel.handle_sound(sound(false)).unwrap(), sound(true));
    let (_dir, mut restore) = sound_handler("", "i32.const 0", true);
    assert_eq!(restore.handle_sound(sound(false)).unwrap(), sound(false));
    assert_eq!(restore.handle_sound(sound(true)).unwrap(), sound(true));
}

#[test]
fn sound_handler_changes_only_cancellation() {
    let (_dir, mut host) = load(
        include_str!("sound.wat"),
        &[
            ("$BODY", ""),
            ("$LOOPING", "i32.const 1"),
            ("$CANCELLED", "local.get 7"),
        ],
        ModGrants {
            audio: true,
            ..Default::default()
        },
    );
    assert_eq!(host.handle_sound(sound(false)).unwrap(), sound(false));
}

#[test]
fn sound_handler_quarantines_traps_imports_and_runaway_calls() {
    for body in [
        "unreachable",
        "call $fuel drop",
        "(loop $forever br $forever)",
    ] {
        let (_dir, mut host) = sound_handler(body, "i32.const 1", true);
        assert!(host.handle_sound(sound(false)).is_err());
        assert!(!host.has_sound_handler());
        assert!(!host.is_active());
    }
}

#[test]
fn sound_handler_bounds_inputs_before_entering_guest() {
    let (_dir, mut host) = sound_handler("", "i32.const 1", true);
    let mut oversized = sound(false);
    oversized.name = "a".repeat(mod_api::MAX_SOUND_NAME_BYTES + 1);
    assert!(host.handle_sound(oversized).is_err());
    let mut invalid = sound(false);
    invalid.position.as_mut().unwrap().x = f32::NAN;
    assert!(host.handle_sound(invalid).is_err());
    assert!(host.has_sound_handler());
    assert!(host.handle_sound(sound(false)).unwrap().cancelled);
}

/// A teleport handler returning the given rotation and server yaw after running its body.
fn teleport_handler(
    body: &str,
    [yaw, pitch]: [&str; 2],
    camera: bool,
) -> (tempfile::TempDir, ModHost) {
    load(
        include_str!("teleport.wat"),
        &[
            ("$BODY", body),
            ("$SERVER_YAW", "f32.const 7"),
            ("$YAW", yaw),
            ("$PITCH", pitch),
        ],
        ModGrants {
            camera,
            ..Default::default()
        },
    )
}

/// A server teleport facing south-west while the player looks north.
fn teleport() -> TeleportContext {
    let server = TeleportRotation {
        yaw: 45.,
        pitch: 10.,
    };
    TeleportContext {
        mode: TeleportMode::Teleport,
        server,
        presented: TeleportRotation {
            yaw: 180.,
            pitch: -20.,
        },
        rotation: server,
    }
}

#[test]
fn teleport_handler_requires_explicit_export_and_camera_grant() {
    let (_dir, mut host) = teleport_handler("", ["local.get 3", "local.get 4"], false);
    assert!(!host.has_teleport_handler());
    assert!(host.handle_teleport(teleport()).is_err());
    assert!(host.is_active());
}

#[test]
fn teleport_handler_replaces_only_the_rotation() {
    let (_dir, mut host) = teleport_handler("", ["local.get 3", "local.get 4"], true);
    assert!(host.has_teleport_handler());
    let handled = host.handle_teleport(teleport()).unwrap();
    assert_eq!(
        handled,
        TeleportContext {
            rotation: teleport().presented,
            ..teleport()
        }
    );
    let (_dir, mut unchanged) = teleport_handler("", ["local.get 5", "local.get 6"], true);
    assert_eq!(unchanged.handle_teleport(teleport()).unwrap(), teleport());
}

#[test]
fn teleport_handler_quarantines_traps_and_invalid_rotations() {
    for (body, rotation) in [
        ("unreachable", ["local.get 3", "local.get 4"]),
        (
            "(loop $forever br $forever)",
            ["local.get 3", "local.get 4"],
        ),
        ("", ["f32.const nan", "local.get 4"]),
        ("", ["local.get 3", "f32.const 90.5"]),
        ("", ["local.get 3", "f32.const inf"]),
    ] {
        let (_dir, mut host) = teleport_handler(body, rotation, true);
        assert!(host.handle_teleport(teleport()).is_err());
        assert!(!host.has_teleport_handler());
        assert!(!host.is_active());
    }
}

#[test]
fn teleport_handler_may_pass_through_an_out_of_range_server_rotation() {
    let (_dir, mut host) = teleport_handler("", ["local.get 5", "local.get 6"], true);
    let server = TeleportRotation {
        yaw: 45.,
        pitch: 270.,
    };
    let incoming = TeleportContext {
        server,
        rotation: server,
        ..teleport()
    };
    assert_eq!(host.handle_teleport(incoming).unwrap(), incoming);
    assert!(host.is_active());
}
