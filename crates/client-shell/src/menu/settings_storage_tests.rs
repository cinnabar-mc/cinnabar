use super::*;
use launcher_host::settings_storage::entries;
use std::{fs, path::PathBuf};

/// Gives each test a private, canonical data directory.
fn fixture() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "cinnabar-storage-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&path).unwrap();
    fs::canonicalize(path).unwrap()
}

#[test]
fn confirmed_screenshot_delete_only_removes_saved_pngs() {
    let root = fixture();
    let mut layout = launcher::test_support::scratch("screenshots");
    layout.user_data_root = root.clone();
    fs::create_dir_all(layout.screenshots_dir()).unwrap();
    fs::create_dir_all(layout.resource_pack_cache_dir()).unwrap();
    let screenshot = layout.screenshots_dir().join("saved.png");
    let other = layout.screenshots_dir().join("notes.txt");
    let pack = layout
        .resource_pack_cache_dir()
        .join("downloaded-pack.mcpack");
    fs::write(&screenshot, b"png").unwrap();
    fs::write(&other, b"notes").unwrap();
    fs::write(&pack, b"pack").unwrap();
    let mut menu = MenuRuntime::new(true, 2, "Steve".into());
    menu.layout = layout;
    refresh(&mut menu);
    menu.activate_storage(StorageAction::RequestScreenshots);
    assert!(
        screenshot.exists(),
        "request must not delete before confirmation"
    );
    menu.activate_storage(StorageAction::ConfirmDelete);
    wait_storage(&mut menu);
    assert!(!screenshot.exists());
    assert!(other.exists());
    assert!(pack.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn deleting_without_selection_preserves_every_cached_pack() {
    let root = fixture();
    let mut menu = MenuRuntime::new(true, 2, "Steve".into());
    menu.layout.user_data_root = root.clone();
    let cache = menu.layout.resource_pack_cache_dir();
    fs::create_dir_all(&cache).unwrap();
    for name in ["a.mcpack", "b.mcpack"] {
        fs::write(cache.join(name), name).unwrap();
    }
    refresh(&mut menu);
    menu.activate_storage(StorageAction::Select(0));
    menu.activate_storage(StorageAction::Select(0));
    menu.activate_storage(StorageAction::RequestDelete);
    assert_ne!(menu.dialog, Some(MenuDialog::StorageDelete));
    menu.activate_storage(StorageAction::ConfirmDelete);
    wait_storage(&mut menu);
    assert_eq!(entries(&cache).unwrap().len(), 2);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn individual_delete_keeps_the_requested_target_after_selection_changes() {
    let root = fixture();
    let mut menu = MenuRuntime::new(true, 2, "Steve".into());
    menu.layout.user_data_root = root.clone();
    let cache = menu.layout.resource_pack_cache_dir();
    fs::create_dir_all(&cache).unwrap();
    for name in ["a.mcpack", "b.mcpack"] {
        fs::write(cache.join(name), name).unwrap();
    }
    refresh(&mut menu);
    let requested = menu.storage.cached[0].path.clone();
    menu.activate_storage(StorageAction::Select(0));
    menu.activate_storage(StorageAction::RequestDelete);
    menu.activate_storage(StorageAction::Select(0));
    menu.activate_storage(StorageAction::ConfirmDelete);
    wait_storage(&mut menu);
    assert!(!requested.exists());
    assert_eq!(entries(&cache).unwrap().len(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn pack_delete_and_clear_all_preserve_imports_and_cache_housekeeping() {
    let root = fixture();
    let mut menu = MenuRuntime::new(true, 2, "Steve".into());
    menu.layout.user_data_root = root.clone();
    let cache = menu.layout.resource_pack_cache_dir();
    let imported = menu.layout.global_resource_packs_dir();
    fs::create_dir_all(&cache).unwrap();
    fs::create_dir_all(&imported).unwrap();
    fs::write(imported.join("keep.mcpack"), b"imported").unwrap();
    fs::write(cache.join(".packcache.lock"), b"lease").unwrap();
    for name in ["first.mcpack", "second.mcpack"] {
        fs::write(cache.join(name), name).unwrap();
    }
    refresh(&mut menu);
    let requested = menu.storage.cached[1].path.clone();
    menu.activate_storage(StorageAction::DeletePack(1));
    let summary = menu.storage.delete_summary.clone();
    assert!(summary.contains(&menu.storage.cached[1].name));
    assert!(requested.exists());
    menu.storage_query.set_text("different search");
    menu.activate_storage(StorageAction::ConfirmDelete);
    wait_storage(&mut menu);
    assert!(!requested.exists());
    assert_eq!(menu.storage.cached.len(), 1);
    menu.activate_storage(StorageAction::RequestClear);
    assert!(menu.storage.delete_summary.contains("All 1"));
    menu.activate_storage(StorageAction::ConfirmDelete);
    wait_storage(&mut menu);
    assert!(menu.storage.cached.is_empty());
    assert!(cache.join(".packcache.lock").exists());
    assert!(imported.join("keep.mcpack").exists());
    menu.activate_storage(StorageAction::RequestClear);
    assert_ne!(menu.dialog, Some(MenuDialog::StorageDelete));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn connected_session_refuses_pack_deletion() {
    let root = fixture();
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.layout.user_data_root = root.clone();
    let cache = menu.layout.resource_pack_cache_dir();
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("pack.mcpack"), b"pack").unwrap();
    refresh(&mut menu);
    menu.session.owns_directory = true;
    menu.activate_storage(StorageAction::DeletePack(0));
    assert_eq!(menu.dialog, Some(MenuDialog::StorageError));
    menu.activate_storage(StorageAction::ConfirmDelete);
    wait_storage(&mut menu);
    assert!(cache.join("pack.mcpack").exists());
    menu.session.owns_directory = false;
    menu.activate_storage(StorageAction::DeletePack(0));
    menu.activate(launcher::menu::MenuAction::DismissDialog);
    menu.activate_storage(StorageAction::ConfirmDelete);
    wait_storage(&mut menu);
    assert!(cache.join("pack.mcpack").exists());
    fs::remove_dir_all(root).unwrap();
}

/// Drives the nonblocking inventory publication after a request or confirmed deletion.
fn wait_storage(menu: &mut MenuRuntime) {
    test_time::eventually("storage inventory", || {
        menu.poll_storage();
        !menu.storage.loading
    });
}

/// Requests a scan and waits for its completed inventory before exercising row actions.
fn refresh(menu: &mut MenuRuntime) {
    menu.refresh_storage();
    wait_storage(menu);
}

#[test]
fn returning_from_world_storage_consumes_the_transition_before_scan_completion() {
    let root = fixture();
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.layout.user_data_root = root.clone();
    Arc::make_mut(&mut menu.storage).return_from_world = true;
    menu.finish_storage_world(launcher::local_worlds::Screen::List);
    assert!(!menu.storage.return_from_world);
    assert!(menu.storage.loading);
    menu.finish_storage_world(launcher::local_worlds::Screen::List);
    wait_storage(&mut menu);
    menu.finish_storage_world(launcher::local_worlds::Screen::List);
    assert!(!menu.storage.loading);
    fs::remove_dir_all(root).unwrap();
}
