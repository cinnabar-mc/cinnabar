//! Play tab shortcuts leave local-world startup and its return page alone.

use super::*;
use launcher::menu::{MenuAction, MenuScreen};

#[test]
fn tab_shortcuts_during_local_world_startup_preserve_the_worlds_return_page() {
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Play));
    menu.local_ui.view.screen = launcher::local_worlds::model::Screen::Opening;
    menu.local_ui.view.progress = Some(launcher::local_worlds::progress::progress(
        None,
        "Local fixture",
    ));
    assert!(!menu.is_connecting());
    menu.step_play_tab(1);
    assert_eq!(menu.screen(), MenuScreen::Play);
    menu.request_local_world_join("Local fixture".into(), false);
    assert!(menu.take_join_intent().is_some());
    menu.show_world();
    menu.open_pause();
    menu.show_after_disconnect();
    assert_eq!(menu.screen(), MenuScreen::Play);
}
