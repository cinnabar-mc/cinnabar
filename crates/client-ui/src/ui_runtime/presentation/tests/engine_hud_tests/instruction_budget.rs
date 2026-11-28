//! Settled frames of a JSON-UI HUD from an original in-memory carrier, so binding, layout and
//! retained paint run without the installed carrier.

use assets::{
    HudTextureRole, RuntimeUiAssets, UiAtlasPage, UiFile, UiTexturePlacement, encode_ui_catalog,
};

use super::*;

/// Frames counted after one warm-up frame.
const FRAMES: u64 = 60;

/// A survival HUD with the hotbar, hearts and hunger drawn by their native renderers.
const HUD_SCREEN: &str = r#"{"namespace":"hud",
  "hud_screen":{"type":"screen","size":["100%","100%"],"controls":[
    {"hotbar":{"type":"custom","renderer":"hotbar_slots_renderer","size":[182,22],
      "anchor_from":"bottom_middle","anchor_to":"bottom_middle"}},
    {"hearts":{"type":"custom","renderer":"heart_renderer","size":[81,9],
      "anchor_from":"bottom_middle","anchor_to":"bottom_middle","offset":[-50,-24]}},
    {"hunger":{"type":"custom","renderer":"hunger_renderer","size":[81,9],
      "anchor_from":"bottom_middle","anchor_to":"bottom_middle","offset":[50,-24]}}
  ]}
}"#;

/// A UI carrier holding only the HUD screen and a flat sprite for every HUD texture role.
fn hud_carrier() -> Arc<RuntimeUiAssets> {
    let files = [
        ("ui/_global_variables.json", "{}"),
        ("ui/_ui_defs.json", r#"{"ui_defs":["ui/hud_screen.json"]}"#),
        ("ui/hud_screen.json", HUD_SCREEN),
    ]
    .map(|(path, text)| UiFile {
        path: path.into(),
        bytes: text.as_bytes().into(),
    });
    // Every HUD texture role resolves, so settled frames never retry missing artwork under the
    // atlas's wall-clock decode deadline.
    let mut sprites: Vec<_> = HudTextureRole::ALL
        .iter()
        .map(|role| role.source_path().trim_end_matches(".png"))
        .collect();
    sprites.sort_unstable();
    sprites.dedup();
    let placements: Vec<_> = sprites
        .into_iter()
        .map(|path| UiTexturePlacement {
            path: path.into(),
            page: 0,
            x: 0,
            y: 0,
            width: 9,
            height: 9,
        })
        .collect();
    let page = UiAtlasPage {
        width: 16,
        height: 16,
        rgba8: vec![255; 16 * 16 * 4].into(),
    };
    let bytes = encode_ui_catalog([3; 32], &[page], &placements, &[], &files).unwrap();
    Arc::new(RuntimeUiAssets::decode(&bytes).unwrap())
}

#[test]
fn instruction_budget_settled_json_ui_hud_frames() {
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    presentation.enable_json_ui_inline(hud_carrier()).unwrap();
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    player.inventory.set_local_selected_slot(0);
    full_stats(&mut player, &mut runtime, 1);
    *presentation.hud_frame_mut() = first_person();
    build(&player, &mut presentation, &runtime, 0);
    let nodes = presentation.hud_draw_nodes();
    assert!(!customs(nodes, HOTBAR_SLOTS).is_empty(), "the hotbar draws");
    assert!(
        !customs(nodes, "heart_renderer").is_empty(),
        "the hearts draw"
    );
    test_instructions::count(|| {
        for frame in 1..=FRAMES {
            std::hint::black_box(build(&player, &mut presentation, &runtime, frame * 16));
        }
    });
    assert!(
        !customs(presentation.hud_draw_nodes(), HOTBAR_SLOTS).is_empty(),
        "the hotbar still draws"
    );
}
