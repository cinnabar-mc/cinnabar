use super::*;

/// Loads an audio-only component with one exact suppression name.
fn load(init: &str, frame: &str, granted: bool) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audio.wat");
    let source = include_str!("audio.wat")
        .replace("$ERROR", if granted { "0" } else { "1" })
        .replace("$INIT", init)
        .replace("$FRAME", frame);
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

#[test]
fn suppression_import_requires_its_own_grant() {
    let (_dir, mut host) = load("call $on", "call $off", false);
    host.frame(false).unwrap();
    assert!(host.muted_sounds().is_empty());
    assert!(host.is_active());
}

#[test]
fn suppression_commits_retains_and_clears_through_component_imports() {
    let (_dir, mut host) = load("call $on", "", true);
    assert_eq!(host.muted_sounds(), ["fixture.one"]);
    host.frame(false).unwrap();
    assert_eq!(host.muted_sounds(), ["fixture.one"]);
    let (_dir, mut host) = load("call $on", "call $off", true);
    host.frame(false).unwrap();
    assert!(host.muted_sounds().is_empty());
}

#[test]
fn suppression_is_released_after_a_failed_callback() {
    let (_dir, mut host) = load("call $on", "call $off unreachable", true);
    assert_eq!(host.muted_sounds(), ["fixture.one"]);
    assert!(host.frame(false).is_err());
    assert!(host.muted_sounds().is_empty());
}
