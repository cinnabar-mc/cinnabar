//! The emote wheel frames each slot's live preview around the whole previewed dance.
use super::super::super::player_preview::{PREVIEW_HEIGHT, PREVIEW_WIDTH, PreviewView, Rig};
use super::tests::{player_catalog_geometry, player_stream};
use super::*;
use std::sync::Arc;

/// An original biped whose every limb has a cube, so its posed silhouette is measurable.
const BIPED: &[u8] = br#"{"format_version":"1.12.0","minecraft:geometry":[{
    "description":{"identifier":"geometry.test_player","texture_width":64,"texture_height":64},"bones":[
    {"name":"root","pivot":[0,0,0]},
    {"name":"waist","parent":"root","pivot":[0,12,0]},
    {"name":"body","parent":"waist","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]}]},
    {"name":"head","parent":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]},
    {"name":"leftArm","parent":"body","pivot":[5,22,0],"cubes":[{"origin":[4,12,-2],"size":[4,12,4],"uv":[32,48]}]},
    {"name":"rightArm","parent":"body","pivot":[-5,22,0],"cubes":[{"origin":[-8,12,-2],"size":[4,12,4],"uv":[40,16]}]},
    {"name":"leftLeg","parent":"root","pivot":[1.9,12,0],"cubes":[{"origin":[-0.1,0,-2],"size":[4,12,4],"uv":[16,48]}]},
    {"name":"rightLeg","parent":"root","pivot":[-1.9,12,0],"cubes":[{"origin":[-3.9,0,-2],"size":[4,12,4],"uv":[0,16]}]}]}]}"#;

/// A synthetic clip that tips the whole body from standing onto its side, as floor moves do.
fn lie_down() -> cosmetics::emotes::Item {
    let half = std::f64::consts::FRAC_1_SQRT_2;
    cosmetics::test_support::clip_emote(
        "fixture:lie_down",
        "Lie down",
        true,
        &serde_json::json!({
            "version": 1,
            "duration": 1.0,
            "loop": true,
            "tracks": [{"joint": "root", "keys": [
                {"time": 0.0, "rotation": [0.0, 0.0, 0.0, 1.0], "offset": [0.0, 0.0, 0.0]},
                {"time": 0.5, "rotation": [0.0, 0.0, half, half], "offset": [0.0, 0.0, 0.0]},
                {"time": 1.0, "rotation": [0.0, 0.0, 0.0, 1.0], "offset": [0.0, 0.0, 0.0]}
            ]}]
        }),
    )
}

/// Where each captured vertex lands when the preview raster is drawn over `bounds`.
fn drawn(vertices: &[render_model::ActorVertex], bounds: ui::UiRect) -> Vec<[f32; 2]> {
    let rig = Rig::new(Default::default(), PreviewView::Hud, 0.0, [false; 2]);
    vertices
        .iter()
        .map(|vertex| {
            let [x, y] = rig.project(*vertex).screen;
            [
                bounds.min().x() + x / PREVIEW_WIDTH as f32 * bounds.width(),
                bounds.min().y() + y / PREVIEW_HEIGHT as f32 * bounds.height(),
            ]
        })
        .collect()
}

fn inside(slot: ui::UiRect, [x, y]: [f32; 2]) -> bool {
    const EPSILON: f32 = 1e-3;
    x >= slot.min().x() - EPSILON
        && x <= slot.max().x() + EPSILON
        && y >= slot.min().y() - EPSILON
        && y <= slot.max().y() + EPSILON
}

#[test]
fn emote_wheel_previews_keep_a_dance_that_lies_down_inside_its_slot() {
    let catalog = player_catalog_geometry(BIPED.to_vec());
    let stream = player_stream(Arc::clone(&catalog), catalog);
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let emote = lie_down();
    let slot = ui::UiRect::new(
        ui::UiPoint::new(100.0, 40.0).unwrap(),
        ui::UiPoint::new(160.0, 100.0).unwrap(),
    )
    .unwrap();
    let mut framed = None;
    let mut cut_off_by_standing_framing = false;
    for step in 0..=8 {
        let phase = emote.duration_seconds() * f64::from(step) / 8.0;
        presentation.capture_hud_player_with_emote_reference(
            Some(stream.authority().local_player_ui()),
            false,
            Some((emote, phase)),
        );
        let vertices = presentation.gui_models.live_player.vertices.clone();
        assert!(!vertices.is_empty(), "the posed preview has geometry");
        let bounds = presentation
            .emote_preview_bounds(slot)
            .expect("a captured emote frames its slot");
        // The frame comes from the whole clip, so it holds still while the dance moves.
        assert_eq!(*framed.get_or_insert(bounds), bounds, "phase {phase}");
        for point in drawn(&vertices, bounds) {
            assert!(
                inside(slot, point),
                "phase {phase}: {point:?} leaves {slot:?}"
            );
        }
        cut_off_by_standing_framing |= drawn(&vertices, slot)
            .into_iter()
            .any(|point| !inside(slot, point));
    }
    assert!(
        cut_off_by_standing_framing,
        "the HUD doll's standing framing cuts this dance off, so the slot needs its own"
    );
}

/// A synthetic clip that steps far aside for a moment shorter than the even sample spacing.
fn quick_side_step() -> cosmetics::emotes::Item {
    let rest = serde_json::json!([0.0, 0.0, 0.0, 1.0]);
    cosmetics::test_support::clip_emote(
        "fixture:quick_side_step",
        "Quick side step",
        true,
        &serde_json::json!({
            "version": 1,
            "duration": 1.0,
            "loop": true,
            "tracks": [{"joint": "root", "keys": [
                {"time": 0.0, "rotation": rest, "offset": [0.0, 0.0, 0.0]},
                {"time": 0.01, "rotation": rest, "offset": [3.0, 0.0, 0.0]},
                {"time": 0.02, "rotation": rest, "offset": [0.0, 0.0, 0.0]},
                {"time": 1.0, "rotation": rest, "offset": [0.0, 0.0, 0.0]}
            ]}]
        }),
    )
}

#[test]
fn emote_wheel_previews_keep_a_brief_excursion_between_even_samples_inside_the_slot() {
    let catalog = player_catalog_geometry(BIPED.to_vec());
    let stream = player_stream(Arc::clone(&catalog), catalog);
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let emote = quick_side_step();
    let slot = ui::UiRect::new(
        ui::UiPoint::new(0.0, 0.0).unwrap(),
        ui::UiPoint::new(60.0, 60.0).unwrap(),
    )
    .unwrap();
    for phase in [0.0, 0.005, 0.01, 0.015] {
        presentation.capture_hud_player_with_emote_reference(
            Some(stream.authority().local_player_ui()),
            false,
            Some((emote, phase)),
        );
        let bounds = presentation.emote_preview_bounds(slot).unwrap();
        let vertices = presentation.gui_models.live_player.vertices.clone();
        for point in drawn(&vertices, bounds) {
            assert!(
                inside(slot, point),
                "phase {phase}: {point:?} leaves {slot:?}"
            );
        }
    }
}

#[test]
fn emote_previews_fit_the_box_centred_without_stretching() {
    let slot = ui::UiRect::new(
        ui::UiPoint::new(0.0, 0.0).unwrap(),
        ui::UiPoint::new(100.0, 50.0).unwrap(),
    )
    .unwrap();
    let content = [glam::Vec2::new(10.0, 20.0), glam::Vec2::new(30.0, 30.0)];
    let bounds = fit_preview_raster(slot, content).unwrap();
    let scale = bounds.width() / PREVIEW_WIDTH as f32;
    assert!((bounds.height() / PREVIEW_HEIGHT as f32 - scale).abs() < 1e-4);
    let box_min = [
        bounds.min().x() + content[0].x * scale,
        bounds.min().y() + content[0].y * scale,
    ];
    let box_max = [
        bounds.min().x() + content[1].x * scale,
        bounds.min().y() + content[1].y * scale,
    ];
    // The 20 by 10 box fills the slot's limiting height, less the margin, about its centre.
    assert!((box_max[1] - box_min[1] - 50.0 * (1.0 - EMOTE_PREVIEW_MARGIN)).abs() < 1e-3);
    assert!(((box_min[0] + box_max[0]) * 0.5 - 50.0).abs() < 1e-3);
    assert!(((box_min[1] + box_max[1]) * 0.5 - 25.0).abs() < 1e-3);
    assert_eq!(fit_preview_raster(slot, [content[0], content[0]]), None);
}
