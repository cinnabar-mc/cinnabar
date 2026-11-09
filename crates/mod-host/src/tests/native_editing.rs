use super::*;

/// Loads a guest whose frame callback traps unless the import reports `expect`.
fn load(expect: bool) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("native_editing.wat");
    let source =
        include_str!("native_editing.wat").replace("$EXPECT", &u8::from(expect).to_string());
    std::fs::write(&path, source).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            controls: true,
            ..Default::default()
        },
    )
    .unwrap();
    (directory, host)
}

/// The supplied state reaches the guest's frame callback and lasts only for that frame.
#[test]
fn native_editing_reaches_the_frame_callback_for_one_frame() {
    let (_dir, mut editing) = load(true);
    editing.set_native_editing(true);
    editing
        .frame_with_controls(false, None, empty_controls())
        .unwrap();
    assert!(
        editing
            .frame_with_controls(false, None, empty_controls())
            .is_err()
    );
    let (_dir, mut idle) = load(false);
    idle.frame_with_controls(false, None, empty_controls())
        .unwrap();
    idle.set_native_editing(false);
    idle.frame_with_controls(false, None, empty_controls())
        .unwrap();
}
