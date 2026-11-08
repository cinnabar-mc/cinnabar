//! The fallback creative search field draws its caret and selection where editing puts them.

use super::*;
use crate::ui_runtime::presentation::forms::pack_harness::drawn_texts;
use semantic_input::KeyboardKey;
use ui::{DpiScale, UiVisual};

/// A font with the glyphs the field draws, so drawn text reads back as typed.
fn field_font() -> Arc<RuntimeFontCatalog> {
    let pixels = vec![255; 4].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/page.png".into(),
        source_bytes: 4,
        source_sha256: [1; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: 1,
        height: 1,
        pixels: FontPixels::Rgba8(pixels.into()),
    };
    let glyphs =
        [json_ui::CARET_GLYPH, 'i', 'n', 'o', 'r', '\u{fffd}'].map(|codepoint| GlyphMetrics {
            codepoint,
            page: 0,
            uv: [0, 0, 1, 1],
            bearing: [0, 0],
            advance_64: 6 * 64,
        });
    let manifest = [5; 32];
    let bytes = encode_font_catalog(manifest, &glyphs, &[page]).unwrap();
    Arc::new(RuntimeFontCatalog::decode(&bytes, manifest).unwrap())
}

/// The texts and solid quads of a fallback frame.
fn drawn(
    presentation: &mut UiPresentationRuntime,
    player: &player_state::PlayerState,
    runtime: &UiRuntime,
) -> (Vec<String>, usize) {
    presentation
        .build(player, runtime, 0, [1280, 720], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert!(presentation.engine_container_frame().is_none());
    let nodes = &presentation.last_frame.as_ref().expect("a frame").nodes;
    let solids = nodes
        .iter()
        .filter(|node| matches!(node.visual(), UiVisual::Solid { .. }))
        .count();
    (drawn_texts(nodes), solids)
}

#[test]
fn fallback_search_draws_its_caret_and_selection_at_the_edit_point() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&protocol::InventoryEvent::Creative(
            protocol::CreativeContentEvent {
                groups: Vec::new().into(),
                items: Vec::new().into(),
                skipped: 0,
            },
        ));
    runtime.toggle_inventory(&mut player, None);
    assert!(runtime.inventory_open());
    let mut presentation = UiPresentationRuntime::with_hud(field_font(), fixture_hud()).unwrap();
    let state = runtime.screen_state_mut();
    state.select_tab(ui_screens::container_actions::SEARCH_TAB);
    state.search_focused = true;
    state.type_text("iron");
    assert!(state.edit_text_key(KeyboardKey::ArrowLeft, false, false));

    let (texts, caret_solids) = drawn(&mut presentation, &player, &runtime);
    assert!(
        texts.iter().any(|text| text == "iro_n"),
        "the caret sits before the last letter: {texts:?}"
    );

    assert!(runtime.screen_state_mut().select_all_text());
    let (texts, selection_solids) = drawn(&mut presentation, &player, &runtime);
    assert!(
        texts.iter().any(|text| text == "iron"),
        "a selection draws the text without a caret: {texts:?}"
    );
    assert_eq!(
        selection_solids,
        caret_solids + 1,
        "the selection draws one highlight behind the text"
    );
}
