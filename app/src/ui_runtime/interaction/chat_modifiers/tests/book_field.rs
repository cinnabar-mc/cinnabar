//! A writable book's page and title edit at their caret and take the selection, copy, cut and
//! paste shortcuts.

use super::*;
use client_ui::ui_runtime::book_screen::{BookSource, BookState};

/// A harness showing a writable book open at the first of `pages`.
fn book_harness(pages: &[&str]) -> Harness {
    let mut h = Harness::new();
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = client_ui::test_support::inventory_session(&mut player);
    runtime.open_book(BookState::new(
        BookSource::Held(0),
        pages.iter().map(|page| (*page).to_owned()).collect(),
        true,
        String::new(),
        String::new(),
    ));
    assert!(runtime.screen_state().text_focused());
    h.app
        .insert_resource(player)
        .insert_resource(UiRuntimeResource(runtime));
    h
}

fn book(h: &Harness) -> BookState {
    h.app
        .world()
        .resource::<UiRuntimeResource>()
        .screen_state()
        .book
        .clone()
        .expect("the book stays open")
}

#[test]
fn book_page_and_title_edit_at_the_caret_with_selection_and_clipboard_shortcuts() {
    let mut h = book_harness(&["dear diary", "next"]);
    h.press(KeyCode::Home, None);
    h.press(KeyCode::KeyM, Some("my "));
    h.press(KeyCode::Enter, Some("\r"));
    assert_eq!(book(&h).pages, ["my \ndear diary", "next"]);

    // Control moves by words and Shift extends the selection; Control+X cuts it.
    h.press(KeyCode::ControlLeft, None);
    h.press(KeyCode::ArrowRight, None);
    h.release(KeyCode::ControlLeft);
    // The arrows move the caret instead of turning the page.
    assert_eq!(book(&h).page, 0);
    h.press(KeyCode::ShiftLeft, None);
    h.press(KeyCode::End, None);
    h.release(KeyCode::ShiftLeft);
    h.press(KeyCode::ControlLeft, None);
    h.press(KeyCode::KeyX, Some("x"));
    assert_eq!(*h.clipboard.lock().unwrap(), "diary");
    assert_eq!(book(&h).pages, ["my \ndear ", "next"]);

    // Control+A then Control+V replaces the whole page, keeping only what the page holds.
    *h.clipboard.lock().unwrap() = "w".repeat(300);
    h.press(KeyCode::KeyA, Some("a"));
    h.press(KeyCode::KeyV, Some("v"));
    h.release(KeyCode::ControlLeft);
    assert_eq!(book(&h).pages[0], "w".repeat(256));

    // The title prompt takes the same keys within its 16 characters.
    h.app
        .world_mut()
        .resource_mut::<UiRuntimeResource>()
        .screen_state_mut()
        .book
        .as_mut()
        .expect("the book stays open")
        .signing = true;
    h.press(KeyCode::KeyB, Some("book"));
    h.press(KeyCode::Home, None);
    h.press(KeyCode::KeyM, Some("my "));
    h.press(KeyCode::ControlLeft, None);
    h.press(KeyCode::KeyA, Some("a"));
    h.press(KeyCode::KeyC, Some("c"));
    h.release(KeyCode::ControlLeft);
    assert_eq!(book(&h).title, "my book");
    assert_eq!(*h.clipboard.lock().unwrap(), "my book");
    h.press(KeyCode::KeyL, Some("a much longer title"));
    assert_eq!(book(&h).title, "a much longer ti");
}
