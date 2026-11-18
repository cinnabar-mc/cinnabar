use super::*;

/// A writable book's page draws its caret where typing goes and follows caret keys; a selection
/// across its lines draws highlighted on each line instead.
#[test]
fn writable_book_page_draws_its_caret_and_selection_at_the_edit_point() {
    use super::super::super::forms::pack_harness::{drawn_texts, font};
    use semantic_input::KeyboardKey;

    let Some(mut presentation) = engine_presentation_with(font()) else {
        eprintln!(
            "skipping writable_book_page_draws_its_caret_and_selection_at_the_edit_point: missing local UI carrier; make assets"
        );
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = book(&mut player, false);
    runtime
        .screen_state_mut()
        .book
        .as_mut()
        .expect("the book is open")
        .pages[0] = "Quill\nInk".to_owned();
    let dpi = DpiScale::new(1.0).unwrap();
    let mut drawn = |runtime: &UiRuntime, now: u64| {
        presentation
            .build(&player, runtime, now, [1280, 720], dpi)
            .unwrap();
        let nodes = &presentation.last_frame.as_ref().expect("a frame").nodes;
        let highlights = nodes
            .iter()
            .filter(|node| matches!(node.visual(), ui::UiVisual::InvertedSprite { .. }))
            .count();
        (drawn_texts(nodes), highlights)
    };
    let caret = json_ui::CARET_GLYPH;
    let state = runtime.screen_state_mut();
    assert!(state.edit_text_key(KeyboardKey::Home, false, false));
    let (texts, unselected) = drawn(&runtime, 1_000);
    assert!(texts.contains(&format!("{caret}QuillInk")), "{texts:?}");
    assert!(
        runtime
            .screen_state_mut()
            .edit_text_key(KeyboardKey::End, false, false)
    );
    let (texts, _) = drawn(&runtime, 2_000);
    assert!(texts.contains(&format!("QuillInk{caret}")), "{texts:?}");
    assert!(runtime.screen_state_mut().select_all_text());
    let (texts, highlights) = drawn(&runtime, 3_000);
    assert!(texts.iter().any(|text| text == "QuillInk"), "{texts:?}");
    assert!(!texts.iter().any(|text| text.contains(caret)), "{texts:?}");
    assert_eq!(
        highlights,
        unselected + 2,
        "one highlight per selected line"
    );
}
