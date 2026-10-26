use super::files::publish_component;
use std::fs;

/// Android forbids hard links, but complete bytes must still reach a new mod file.
#[test]
fn publishing_installs_complete_bytes_and_removes_staging() {
    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    let bytes = b"complete mod bytes";
    publish_component(&mods, "zoom.wasm", bytes).unwrap();
    assert_eq!(fs::read(mods.join("zoom.wasm")).unwrap(), bytes);
    assert_eq!(fs::read_dir(&mods).unwrap().count(), 1);
}

/// Existing links cannot redirect staging writes or be replaced at publication.
#[cfg(unix)]
#[test]
fn publishing_never_overwrites_destination_or_staging_links() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let mods = directory.path().join("mods");
    fs::create_dir_all(&mods).unwrap();
    let target = directory.path().join("outside.wasm");
    let destination = mods.join("zoom.wasm");
    symlink(&target, &destination).unwrap();
    assert!(publish_component(&mods, "zoom.wasm", b"replacement").is_err());
    assert_eq!(fs::read_link(&destination).unwrap(), target);
    assert!(!target.exists());

    fs::remove_file(&destination).unwrap();
    fs::write(&target, b"original").unwrap();
    let staging = mods.join(format!(".zoom.wasm.{}.importing", std::process::id()));
    symlink(&target, &staging).unwrap();
    assert!(publish_component(&mods, "zoom.wasm", b"replacement").is_err());
    assert_eq!(fs::read(&target).unwrap(), b"original");
    assert_eq!(fs::read_link(&staging).unwrap(), target);
    assert!(!destination.exists());
}
