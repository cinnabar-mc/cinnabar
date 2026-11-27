//! Settled frames of a JSON-UI HUD built from an original in-memory layer, so binding, layout and
//! retained paint run without the installed carrier.

use super::*;

/// Frames counted after one warm-up frame.
const FRAMES: u64 = 60;

/// A survival HUD with the hotbar, hearts and hunger drawn by their native renderers.
const HUD_LAYER: &[u8] = br#"{"namespace":"hud",
  "hud_screen":{"type":"screen","size":["100%","100%"],"controls":[
    {"hotbar":{"type":"custom","renderer":"hotbar_slots_renderer","size":[182,22],
      "anchor_from":"bottom_middle","anchor_to":"bottom_middle"}},
    {"hearts":{"type":"custom","renderer":"heart_renderer","size":[81,9],
      "anchor_from":"bottom_middle","anchor_to":"bottom_middle","offset":[-50,-24]}},
    {"hunger":{"type":"custom","renderer":"hunger_renderer","size":[81,9],
      "anchor_from":"bottom_middle","anchor_to":"bottom_middle","offset":[50,-24]}}
  ]}
}"#;

#[test]
fn instruction_budget_settled_json_ui_hud_frames() {
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    presentation
        .enable_json_ui_inline(crate::test_support::mini_carrier())
        .unwrap();
    presentation.set_server_ui_pack(&crate::ui_runtime::presentation::forms::ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec(),
            ),
            ("ui/hud_screen.json".into(), HUD_LAYER.to_vec()),
        ]],
        ..Default::default()
    });
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
