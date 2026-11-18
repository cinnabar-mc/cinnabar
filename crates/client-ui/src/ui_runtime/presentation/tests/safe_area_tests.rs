//! Safe-area inset witnesses: platform insets flow into the HUD geometry,
//! the retained layout, and the render viewport, and viewports whose safe
//! region cannot hold the fixed HUD fail closed to no HUD at all.

use protocol::PlayerGameMode;
use ui::SafeArea;

use super::{fixture_font, fixture_hud};
use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::presentation::{HudFrame, UiPresentationRuntime};

fn insets(left: f32, top: f32, right: f32, bottom: f32) -> SafeArea {
    SafeArea::new(left, top, right, bottom).unwrap()
}

fn hud_presentation(preference: Option<u8>, safe_area: SafeArea) -> UiPresentationRuntime {
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    presentation.set_gui_scale_preference(preference);
    presentation.set_safe_area(safe_area);
    *presentation.hud_frame_mut() = HudFrame {
        first_person: true,
        ..HudFrame::default()
    };
    presentation
}

#[test]
fn too_short_or_over_inset_viewports_fail_closed_to_no_hud() {
    let mut player_runtime = player_state::PlayerState::new(1);

    // (physical, dpi, preference, insets): each safe viewport is too narrow
    // or too short for the fixed hotbar and bottom stack.
    for (physical, dpi, preference, safe_area) in [
        // 50 GUI px tall at the auto scale of 1: shorter than the 59 px
        // bottom stack.
        ([1280u32, 50u32], 1.0f32, None, SafeArea::ZERO),
        // Insets consume the height: (720 - 600) / 3 = 40 GUI px.
        ([1280, 720], 1.0, None, insets(0.0, 400.0, 0.0, 200.0)),
        // Insets consume the width: 600 - 430 = 170 < 182 GUI px at k = 1.
        ([600, 720], 1.0, None, insets(250.0, 0.0, 180.0, 0.0)),
    ] {
        let mut presentation = hud_presentation(preference, safe_area);
        let runtime = UiRuntime::new(1);
        player_runtime
            .facts
            .publish_player_game_mode(PlayerGameMode::Survival);
        player_runtime.inventory.set_local_selected_slot(2);
        let input = presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                physical,
                ui::DpiScale::new(dpi).unwrap(),
            )
            .unwrap();
        assert!(
            input.vertices.is_empty(),
            "no HUD renders in an unsafe viewport {physical:?} {safe_area:?}"
        );
    }
}

#[test]
fn render_input_carries_the_physical_safe_area_insets() {
    let player_runtime = player_state::PlayerState::new(1);

    let mut presentation = hud_presentation(None, insets(10.0, 20.0, 30.0, 40.0));
    let runtime = UiRuntime::new(1);
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.5).unwrap(),
        )
        .unwrap();
    assert_eq!(input.viewport_size, [1280, 720]);
    assert_eq!(
        input.safe_area,
        [15, 30, 45, 60],
        "logical insets reach the render viewport as physical px"
    );
}

#[test]
fn crosshair_stays_in_the_world_center_while_edge_hud_respects_insets() {
    use json_ui::vanilla::Reference;
    use serde_json::json;
    use ui_screens::hud_renderers::{CROSSHAIR_SIDE, CROSSHAIR_TEXTURE, CURSOR_RENDERER};

    let physical = [1280, 720];
    let scale = ui::gui_scale(physical, None) as f32;
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    let mut texture = Vec::new();
    image::RgbaImage::from_pixel(8, 8, image::Rgba([255; 4]))
        .write_to(
            &mut std::io::Cursor::new(&mut texture),
            image::ImageFormat::Png,
        )
        .unwrap();
    let (namespace, screen) = Reference::HudCrosshairHudCrosshairScreen
        .as_str()
        .split_once('.')
        .unwrap();
    for cursor in [
        json!({"type":"custom", "renderer":CURSOR_RENDERER}),
        json!({"type":"image", "texture":CROSSHAIR_TEXTURE}),
    ] {
        let mut presentation = hud_presentation(None, SafeArea::ZERO);
        presentation
            .enable_json_ui_inline(crate::test_support::mini_carrier())
            .unwrap();
        let mut cursor = cursor;
        cursor["size"] = json!([CROSSHAIR_SIDE, CROSSHAIR_SIDE]);
        cursor["anchor_from"] = json!("center");
        cursor["anchor_to"] = json!("center");
        let mut runtime = UiRuntime::new(1);
        runtime.set_server_ui(Some(std::sync::Arc::new(
            crate::ui_runtime::presentation::forms::ServerUiPack {
                // Both screens keep vanilla's root flags: the world shows behind them, and
                // neither takes gameplay input.
                ui_layers: vec![vec![
                    (
                        json_ui::UI_DEFS_PATH.into(),
                        json!({
                            "ui_defs":["ui/hud_crosshair_fixture.json", "ui/hud_screen.json"]
                        })
                        .to_string()
                        .into_bytes(),
                    ),
                    (
                        "ui/hud_crosshair_fixture.json".into(),
                        json!({
                            "namespace":namespace, (screen):{"type":"screen",
                                "render_game_behind":true, "render_only_when_topmost":false,
                                "absorbs_input":false, "is_showing_menu":false,
                                "controls":[{"cursor":cursor}]}
                        })
                        .to_string()
                        .into_bytes(),
                    ),
                    (
                        "ui/hud_screen.json".into(),
                        json!({
                            "namespace":"hud", "hud_screen":{"type":"screen",
                                "absorbs_input":false, "is_showing_menu":false, "controls":[{
                                "edge":{"type":"image", "texture":CROSSHAIR_TEXTURE,
                                    "size":[8,8], "anchor_from":"top_left", "anchor_to":"top_left"}
                            }]}
                        })
                        .to_string()
                        .into_bytes(),
                    ),
                ]],
                textures: vec![(format!("{CROSSHAIR_TEXTURE}.png"), texture.clone())],
                ..Default::default()
            },
        )));
        for dpi in [1.0, 1.5, 2.0] {
            for safe_area in [
                SafeArea::ZERO,
                insets(96.0, 0.0, 0.0, 0.0),
                insets(0.0, 0.0, 96.0, 0.0),
                insets(12.0, 48.0, 36.0, 20.0),
            ] {
                presentation.set_safe_area(safe_area);
                // Check both the first paint and its retained replay.
                for now in [0, 1] {
                    let input = presentation
                        .build(
                            &player,
                            &runtime,
                            now,
                            physical,
                            ui::DpiScale::new(dpi).unwrap(),
                        )
                        .unwrap();
                    let bounds = input
                        .vertices
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|quad| {
                            quad.iter().fold(
                                [
                                    f32::INFINITY,
                                    f32::INFINITY,
                                    f32::NEG_INFINITY,
                                    f32::NEG_INFINITY,
                                ],
                                |b, v| {
                                    [
                                        b[0].min(v.position[0]),
                                        b[1].min(v.position[1]),
                                        b[2].max(v.position[0]),
                                        b[3].max(v.position[1]),
                                    ]
                                },
                            )
                        })
                        .collect::<Vec<_>>();
                    let cursor = bounds
                        .iter()
                        .find(|b| (b[2] - b[0] - CROSSHAIR_SIDE * scale).abs() < 0.01)
                        .expect("painted crosshair");
                    assert!(
                        ((cursor[0] + cursor[2]) * 0.5 - physical[0] as f32 * 0.5).abs() < 0.01
                    );
                    assert!(
                        ((cursor[1] + cursor[3]) * 0.5 - physical[1] as f32 * 0.5).abs() < 0.01
                    );
                    let edge = bounds
                        .iter()
                        .find(|b| (b[2] - b[0] - 8.0 * scale).abs() < 0.01)
                        .expect("painted edge HUD control");
                    assert!((edge[0] - safe_area.left() * dpi).abs() < 0.01);
                    assert!((edge[1] - safe_area.top() * dpi).abs() < 0.01);
                }
            }
        }
    }
}

#[test]
fn tiny_launcher_text_keeps_rendering_after_resize() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let player = player_state::PlayerState::new(0);
    let runtime = UiRuntime::new(0);
    let mut view = launcher::menu::MenuView::new(true, "Fixture".into());
    view.screen = launcher::menu::MenuScreen::Settings;
    presentation.set_menu_view(Some(view));
    for physical in [[1280, 720], [254, 124], [1, 1], [1280, 720]] {
        let result = presentation.build(
            &player,
            &runtime,
            0,
            physical,
            ui::DpiScale::new(1.0).unwrap(),
        );
        if physical == [1, 1] {
            assert!(
                result.is_err(),
                "unfit screen geometry rejects only this frame"
            );
            continue;
        }
        let input = result.unwrap();
        assert_eq!(input.viewport_size, physical);
        if physical == [1280, 720] {
            assert!(!input.vertices.is_empty());
        }
        super::super::forms::snapshot::write(
            &input,
            &format!("tiny-text-{}x{}", physical[0], physical[1]),
        );
    }
}

#[test]
fn six_pixel_glyph_in_one_pixel_label_still_has_a_layout() {
    let font = fixture_font();
    let metrics = ui_engine::text_metrics::TextMetrics::for_viewport(
        [254, 124],
        ui::DpiScale::new(1.0).unwrap(),
        None,
    );
    let mut cache = ui::TextLayoutCache::new(8, 64 * 1024);
    let layout = cache.layout(metrics.request("W", 64, &font)).unwrap();
    assert_eq!(layout.size_64()[0], 384);
    assert_eq!(layout.glyphs().len(), 1);
    assert_eq!(cache.visual_overflow_count(), 1);
    cache.layout(metrics.request("W", 64, &font)).unwrap();
    assert_eq!(
        cache.visual_overflow_count(),
        1,
        "cached frames do not recount text"
    );
    let mut strict = metrics.request("W", 64, &font);
    strict.wrap.allow_visual_overflow = false;
    assert!(
        matches!(
            cache.layout(strict),
            Err(ui::TextError::VisualWidthExceeded {
                actual_64: 384,
                limit_64: 64
            })
        ),
        "strict requests do not reuse the tolerant layout"
    );
    let bounds = super::super::rect(0.0, 0.0, 1.0, 10.0).unwrap();
    let viewport = super::super::rect(0.0, 0.0, 254.0, 124.0).unwrap();
    let parent = ui::UiNodeId::new(1);
    let mut tree = ui::UiTree::new(vec![
        ui::UiNode::new(parent, None, bounds).with_clip_children(true),
        ui::UiNode::new(ui::UiNodeId::new(2), Some(parent), bounds).with_visual(
            ui::UiVisual::Text {
                layout,
                color: [255; 4],
                shadow: ui::TextShadow::None,
            },
        ),
    ])
    .unwrap();
    tree.layout(viewport, ui::UiScale::default(), ui::SafeArea::ZERO)
        .unwrap();
    let draw = tree.build_draw_list().unwrap();
    assert!(!draw.vertices.is_empty(), "the wide glyph reaches drawing");
    assert!(draw.batches.iter().all(|batch| batch.clip == bounds));
    let presentation = UiPresentationRuntime::new(font.clone()).unwrap();
    let input = crate::ui_runtime::render_adapter::adapt_ui_draw_list(
        &draw,
        presentation.textures.clone(),
        crate::ui_runtime::render_adapter::UiRenderViewport {
            physical_size: [254, 124],
            dpi_scale: ui::DpiScale::new(1.0).unwrap(),
            safe_area: ui::SafeArea::ZERO,
        },
    )
    .unwrap();
    let pixels = super::super::forms::snapshot::rasterize(&input);
    let background = *pixels.get_pixel(253, 123);
    assert!(
        pixels.pixels().any(|pixel| *pixel != background),
        "clipped glyph ink renders"
    );
    assert!(
        pixels
            .enumerate_pixels()
            .all(|(x, y, pixel)| *pixel == background || (x == 0 && y < 10)),
        "ink stays inside the one-pixel control"
    );
    super::super::forms::snapshot::write(&input, "overflowing-glyph-clipped");
}

#[test]
fn installed_launcher_text_snapshot() {
    let Some(mut presentation) = super::super::forms::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping installed_launcher_text_snapshot: missing local UI carrier (make assets)"
        );
        return;
    };
    let runtime = super::super::forms::pack_harness::menu_runtime();
    let player = player_state::PlayerState::new(0);
    let mut view = launcher::menu::MenuView::new(true, "Fixture".into());
    view.screen = launcher::menu::MenuScreen::Settings;
    presentation.set_menu_view(Some(view));
    let input = presentation
        .build(
            &player,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    super::super::forms::snapshot::write(&input, "installed-launcher-text-normal");
}
