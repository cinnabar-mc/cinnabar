use {
    super::*,
    launcher::local_worlds::model::{Input, Screen},
};

/// Attaching the launcher loads its catalog and preferences without requesting a world start.
#[test]
fn attaching_never_requests_a_saved_world_start() {
    let mut worlds = LocalWorlds::default();
    let effects = worlds.attachment_effects();
    assert!(!effects.is_empty(), "the menu still requests its data");
    assert!(
        effects
            .iter()
            .all(|effect| matches!(effect, Effect::List | Effect::LoadPrefs)),
        "attachment requested work beyond loading menu data: {effects:?}"
    );
}

#[test]
fn input_without_a_client_still_advances_the_model() {
    let mut worlds = LocalWorlds::default();
    worlds.input(Input::BeginCreate);
    assert_eq!(worlds.menu().screen(), Screen::Create);
    worlds.input(Input::Back);
    worlds.input(Input::Refresh);
    assert!(worlds.menu().busy());
}

/// An import asked for before the world worker is attached is refused, not left on its modal.
#[test]
fn an_import_before_attach_is_refused() {
    let mut worlds = LocalWorlds::default();
    worlds.input(Input::Import);
    assert_eq!(worlds.menu().screen(), Screen::Error);
    assert!(!worlds.menu().busy());
    worlds.input(Input::Back);
    assert_eq!(worlds.menu().screen(), Screen::List);
}

/// A detach while an import is pending ends its modal instead of leaving it up for good.
#[test]
fn a_detach_mid_import_ends_the_import_modal() {
    let dir = tempfile::tempdir().expect("worlds folder");
    let manager =
        launcher_host::local_worlds::open_store(dir.path().join("worlds")).expect("manager");
    let mut worlds = LocalWorlds::default();
    worlds.client =
        Some(WorldsClient::spawn(manager, dir.path().join("no-core-here")).expect("worker"));
    worlds.input(Input::ImportFiles(vec![dir.path().join("Home.mcworld")]));
    assert_eq!(worlds.menu().screen(), Screen::Importing);
    worlds.detach();
    assert_eq!(worlds.menu().screen(), Screen::Error);
    assert!(!worlds.menu().busy());
}

/// A saved flat BDS world named `name`.
fn saved(manager: &Manager, name: &str) -> launcher::local_worlds::world::World {
    manager
        .create(&launcher::local_worlds::world::NewWorld {
            name: name.to_owned(),
            generator: launcher::local_worlds::world::Generator::Flat,
            backend: Some(bridge::Backend::Bds),
            ..Default::default()
        })
        .expect("create")
}

/// A manager whose worlds run on an in-memory server and close as soon as they run hidden.
fn closing_when_hidden(dir: &tempfile::TempDir) -> Manager {
    launcher_host::local_worlds::open_store_with_server(
        dir.path().join("worlds"),
        std::time::Duration::ZERO,
    )
    .expect("manager")
}

/// Hiding and showing the app with nothing open reports no closed world and leaves the menu.
#[test]
fn hiding_with_nothing_open_keeps_the_menu() {
    let mut worlds = LocalWorlds::default();
    worlds.app_hidden();
    assert!(!worlds.app_shown());
    let dir = tempfile::tempdir().expect("worlds folder");
    worlds.manager = Some(closing_when_hidden(&dir));
    worlds.app_hidden();
    assert!(!worlds.app_shown());
    assert_eq!(worlds.menu().screen(), Screen::List);
}

/// A played world the host closed while hidden is reported on return; the session in progress is
/// the caller's to end, so the menu stays where it was.
#[test]
fn a_world_closed_while_hidden_is_reported_on_return() {
    let dir = tempfile::tempdir().expect("worlds folder");
    let manager = closing_when_hidden(&dir);
    let world = saved(&manager, "w");
    manager.open(&world.id, None).expect("open");
    test_time::eventually("the world to run", || {
        manager.status().state == bridge::WorldState::Running
    });
    let mut worlds = LocalWorlds::with_manager(manager.clone());
    worlds.app_hidden();
    test_time::eventually("the world to close", || {
        manager.status().state == bridge::WorldState::Idle
    });
    assert!(worlds.app_shown(), "the closed world is reported");
    assert!(!worlds.app_shown(), "once");
    assert_eq!(worlds.menu().screen(), Screen::List);
}

/// An open the host closed while hidden ends on the explanation instead of handing off.
#[test]
fn an_open_closed_while_hidden_ends_on_the_explanation() {
    let dir = tempfile::tempdir().expect("worlds folder");
    let manager = closing_when_hidden(&dir);
    let world = saved(&manager, "w");
    let mut worlds = LocalWorlds::with_manager(manager.clone());
    worlds.client = Some(
        WorldsClient::spawn(manager.clone(), dir.path().join("no-core-here")).expect("worker"),
    );
    worlds.apply(Event::Listed(vec![world]));
    worlds.input(Input::Play);
    assert_eq!(worlds.menu().screen(), Screen::Opening);
    test_time::eventually("the world to start", || {
        manager.status().state != bridge::WorldState::Idle
    });
    worlds.app_hidden();
    test_time::eventually("the world to close", || {
        manager.status().state == bridge::WorldState::Idle
    });
    assert!(worlds.app_shown());
    assert_eq!(worlds.menu().screen(), Screen::Error);
    assert_eq!(
        worlds.menu().error(),
        Some(launcher::local_worlds::model::CLOSED_IN_BACKGROUND)
    );
    worlds.pump();
    assert!(
        worlds.take_ready().is_none(),
        "a closed world must not hand off"
    );
}
