use super::*;

/// Loads a component returning independent cancellation decisions for its first two sounds.
fn load(body: &str, length: &str, granted: bool) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audio.wat");
    let source = include_str!("audio.wat")
        .replace("$BODY", body)
        .replace("$LENGTH", length);
    std::fs::write(&path, source).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            audio: granted,
            ..Default::default()
        },
    )
    .unwrap();
    (directory, host)
}

/// Two independent observations of the same sound can receive different verdicts.
fn events() -> Vec<SoundEvent> {
    [1., 2.]
        .into_iter()
        .map(|x| SoundEvent {
            name: "fixture.sound".into(),
            position: Some(SoundPosition { x, y: 64., z: 0. }),
            looping: false,
        })
        .collect()
}

#[test]
fn sound_filter_requires_explicit_export_and_grant() {
    let (_dir, mut host) = load("", "local.get 1", false);
    assert!(!host.has_sound_filter());
    assert!(host.filter_sounds(events()).is_err());
    assert!(host.is_active());
}

#[test]
fn sound_filter_returns_independent_event_decisions() {
    let (_dir, mut host) = load("", "local.get 1", true);
    assert!(host.has_sound_filter());
    assert_eq!(host.filter_sounds(events()).unwrap(), [false, true]);
    assert_eq!(host.filter_sounds(events()).unwrap(), [false, true]);
}

#[test]
fn sound_filter_quarantines_traps_bad_lengths_imports_and_runaway_callbacks() {
    for (body, length) in [
        ("unreachable", "local.get 1"),
        ("", "i32.const 0"),
        ("call $fuel drop", "local.get 1"),
        ("(loop $forever br $forever)", "local.get 1"),
    ] {
        let (_dir, mut host) = load(body, length, true);
        assert!(host.filter_sounds(events()).is_err());
        assert!(!host.has_sound_filter());
        assert!(!host.is_active());
    }
}

#[test]
fn sound_filter_bounds_inputs_before_entering_guest() {
    let (_dir, mut host) = load("", "local.get 1", true);
    let mut oversized = events();
    oversized[0].name = "a".repeat(mod_api::MAX_SOUND_NAME_BYTES + 1);
    assert!(host.filter_sounds(oversized).is_err());
    assert!(
        host.filter_sounds(vec![
            events()[0].clone();
            mod_api::MAX_SOUND_FILTER_EVENTS + 1
        ])
        .is_err()
    );
    let mut invalid = events();
    invalid[0].position.as_mut().unwrap().x = f32::NAN;
    assert!(host.filter_sounds(invalid).is_err());
    assert!(host.has_sound_filter());
    assert_eq!(host.filter_sounds(events()).unwrap(), [false, true]);
}
