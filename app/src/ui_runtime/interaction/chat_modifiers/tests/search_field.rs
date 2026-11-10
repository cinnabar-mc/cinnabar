//! The inventory's search box edits at its caret and takes the selection, copy, cut and paste
//! shortcuts.

use super::*;
use client_ui::ui_runtime::inventory_drag::PointerAction;
use ui_screens::container_actions::InventoryCellHit;

/// A harness whose inventory is open with its search box focused.
fn search_harness() -> Harness {
    let mut h = Harness::new();
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = client_ui::test_support::inventory_session(&mut player);
    runtime.toggle_inventory(&mut player, None);
    runtime.perform_pointer_action(
        &mut player,
        PointerAction::Click(InventoryCellHit::CreativeSearch),
    );
    assert!(runtime.screen_state().text_focused());
    h.app
        .insert_resource(player)
        .insert_resource(UiRuntimeResource(runtime));
    h
}

fn search(h: &Harness) -> String {
    h.app
        .world()
        .resource::<UiRuntimeResource>()
        .screen_state()
        .search
        .clone()
}

#[test]
fn inventory_search_edits_at_the_caret_with_selection_and_clipboard_shortcuts() {
    let mut h = search_harness();
    h.press(KeyCode::KeyI, Some("iron ingot"));
    h.press(KeyCode::Home, None);
    h.press(KeyCode::KeyR, Some("raw "));
    assert_eq!(search(&h), "raw iron ingot");

    // Shift extends the selection, Control moves by words, and typing replaces the selection.
    h.press(KeyCode::ShiftLeft, None);
    h.press(KeyCode::ControlLeft, None);
    h.press(KeyCode::ArrowRight, None);
    h.release(KeyCode::ControlLeft);
    h.release(KeyCode::ShiftLeft);
    h.press(KeyCode::KeyC, Some("cooked "));
    assert_eq!(search(&h), "raw cooked ingot");
    h.press(KeyCode::End, None);
    h.press(KeyCode::Backspace, Some("\u{8}"));
    h.press(KeyCode::ArrowLeft, None);
    h.press(KeyCode::Delete, Some("\u{7f}"));
    assert_eq!(search(&h), "raw cooked ing");

    // Control+A selects everything, Control+C copies it and Control+X cuts it, without typing.
    h.press(KeyCode::ControlLeft, None);
    h.press(KeyCode::KeyA, Some("a"));
    h.press(KeyCode::KeyC, Some("c"));
    assert_eq!(*h.clipboard.lock().unwrap(), "raw cooked ing");
    assert_eq!(search(&h), "raw cooked ing");
    h.press(KeyCode::KeyX, Some("x"));
    assert_eq!(search(&h), "");
    *h.clipboard.lock().unwrap() = "gold".into();
    h.press(KeyCode::KeyV, Some("v"));
    h.release(KeyCode::ControlLeft);
    assert_eq!(search(&h), "gold");
    assert!(
        h.app
            .world()
            .resource::<UiRuntimeResource>()
            .screen_state()
            .text_focused()
    );
}
