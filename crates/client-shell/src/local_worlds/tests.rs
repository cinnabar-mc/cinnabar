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
