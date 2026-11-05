use super::*;

/// One handler fixture: its source, placeholder substitutions and grants.
type HandlerSpec<'a> = (&'a str, Vec<(&'a str, &'a str)>, ModGrants);

/// Loads one handler per spec against the host's ABI fixtures.
fn handlers(specs: Vec<HandlerSpec>) -> ModHosts {
    let directory = tempfile::tempdir().unwrap();
    let mods = specs
        .into_iter()
        .enumerate()
        .map(|(index, (fixture, substitutions, grants))| {
            let source = substitutions
                .iter()
                .fold(fixture.to_owned(), |source, (from, to)| {
                    source.replace(from, to)
                });
            let path = directory.path().join(format!("handler-{index}.wat"));
            std::fs::write(&path, source).unwrap();
            (path, grants)
        })
        .collect();
    load_set(mods)
}

/// Sound handlers running `body` and then returning `cancelled`, in load order.
fn sound_handlers(bodies: &[(&str, &str)]) -> ModHosts {
    handlers(
        bodies
            .iter()
            .map(|&(body, cancelled)| {
                (
                    include_str!("../../../mod-host/src/tests/sound.wat"),
                    vec![
                        ("$BODY", body),
                        ("$LOOPING", "local.get 6"),
                        ("$CANCELLED", cancelled),
                    ],
                    ModGrants {
                        audio: true,
                        ..Default::default()
                    },
                )
            })
            .collect(),
    )
}

/// Two identical non-positional requests.
fn sounds() -> Vec<mod_host::SoundContext> {
    vec![
        mod_host::SoundContext {
            name: "fixture.sound".into(),
            position: None,
            looping: false,
            cancelled: false,
        };
        2
    ]
}

/// A later handler sees an earlier cancellation and cannot undo it.
#[test]
fn sound_cancellation_is_sticky_across_components() {
    let mut hosts = sound_handlers(&[("", "i32.const 1"), ("", "i32.const 0")]);
    assert_eq!(hosts.handle_sounds(sounds()), [false, false]);
    let mut hosts = sound_handlers(&[("", "local.get 7"), ("", "local.get 7")]);
    assert_eq!(hosts.handle_sounds(sounds()), [true, true]);
}

/// Runaway handlers consume the frame's pool; skipped handlers stay active and run next frame.
#[test]
fn sound_handler_budget_bounds_fanout_without_trapping_skipped_components() {
    let runaway_count = (mod_api::TOTAL_SOUND_HANDLER_FUEL / mod_api::HANDLER_CALL_FUEL) as usize;
    let mut bodies = vec![("(loop $forever br $forever)", "i32.const 1"); runaway_count];
    bodies.push(("", "i32.const 1"));
    let mut hosts = sound_handlers(&bodies);
    assert_eq!(hosts.handle_sounds(sounds()), [true, true]);
    assert!((0..runaway_count).all(|index| !hosts.host(index).is_active()));
    assert!(hosts.host(runaway_count).has_sound_handler());
    assert_eq!(hosts.handle_sounds(sounds()), [false, false]);
}

/// Teleport handlers returning `[yaw, pitch]` expressions over the incoming context.
fn teleport_handlers(rotations: &[[&str; 2]]) -> ModHosts {
    handlers(
        rotations
            .iter()
            .map(|&[yaw, pitch]| {
                (
                    include_str!("../../../mod-host/src/tests/teleport.wat"),
                    vec![
                        ("$BODY", ""),
                        ("$SERVER_YAW", "local.get 1"),
                        ("$YAW", yaw),
                        ("$PITCH", pitch),
                    ],
                    ModGrants {
                        camera: true,
                        ..Default::default()
                    },
                )
            })
            .collect(),
    )
}

/// Bedrock degrees as a teleport rotation.
fn rotation(yaw: f32, pitch: f32) -> mod_host::TeleportRotation {
    mod_host::TeleportRotation { yaw, pitch }
}

/// A server teleport while the player looks elsewhere.
fn teleport() -> mod_host::TeleportContext {
    mod_host::TeleportContext {
        mode: mod_host::TeleportMode::Teleport,
        server: rotation(45., 10.),
        presented: rotation(180., -20.),
        rotation: rotation(45., 10.),
    }
}

/// Each teleport handler receives the previous handler's rotation; a failure keeps it.
#[test]
fn teleport_handlers_chain_rotations_in_load_order() {
    let mut hosts = teleport_handlers(&[
        ["local.get 3", "local.get 4"],
        ["local.get 5 f32.const 1 f32.add", "local.get 6"],
        ["f32.const nan", "local.get 6"],
    ]);
    let handled = hosts.handle_teleport(teleport());
    assert_eq!(handled.rotation, rotation(181., -20.));
    assert_eq!(handled.server, teleport().server);
    assert!(!hosts.host(2).is_active());
    assert!(hosts.host(1).has_teleport_handler());
}
