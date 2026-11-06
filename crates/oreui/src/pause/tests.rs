//! Pause keeps full-height actions and a clear character area in short windows.

use super::*;
use crate::{review_tests::paint, theme::GUI_PIXELS_PER_REM};
use ui_engine::text_metrics::TextMetrics;

#[test]
fn pause_scales_actions_together_at_the_window_minimum() {
    for size in [ui::MIN_WINDOW_LOGICAL_SIZE, [480.0, 270.0], [1000.0, 560.0]] {
        let metrics = TextMetrics::for_viewport(
            size.map(|value| value as u32),
            ui::DpiScale::new(1.0).unwrap(),
            Some(2),
        );
        let rem = metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32 * GUI_PIXELS_PER_REM;
        let view = MenuView::new(true, "Fixture".into());
        let mut preview = None;
        let (_, hits, _) = paint(Default::default(), |c| {
            c.metrics = metrics;
            c.rem = rem;
            c.title_artwork = Some(ui::IconRef {
                color_mask: None,
                page: 18,
                uv: [0, 0, 400, 80],
                glint: false,
            });
            preview = draw(c, &view, size, &|_| None).unwrap();
            assert_eq!(c.rem, rem);
            assert_eq!(c.metrics.scale, metrics.scale);
        });
        let compact = preview.is_none();
        let margin = (rem * 2.4).min(size[0] * 0.04).min(size[1] * 0.025);
        let scale = ((size[1] - margin * 2.0)
            / (rem * minimum_height(compact, true, view.touch_available)))
        .min(1.0);
        for action in [
            MenuAction::PauseResume,
            MenuAction::PauseSettings,
            MenuAction::PauseDisconnect,
            MenuAction::Navigate(MenuScreen::DressingRoom),
        ] {
            let bounds = hits.iter().find(|(found, _)| *found == action).unwrap().1;
            assert!(
                (bounds.height() - rem * scale * ACTION_HEIGHT).abs() < 0.001,
                "{action:?} must share the group scale at {size:?}"
            );
            assert!(bounds.min().x() >= 0.0 && bounds.max().x() <= size[0]);
            assert!(bounds.min().y() >= 0.0 && bounds.max().y() <= size[1]);
        }
        if let Some(preview) = preview {
            assert!(preview.control[3] > preview.control[1]);
            let wardrobe = hits
                .iter()
                .find(|(action, _)| *action == MenuAction::Navigate(MenuScreen::DressingRoom))
                .unwrap()
                .1;
            assert!(preview.control[3] < wardrobe.min().y());
        }
    }
}

#[test]
fn pause_keeps_existing_geometry_at_short_desktop_and_normal_sizes() {
    let view = MenuView::new(true, "Fixture".into());
    for size in [[1000.0, 560.0], [1920.0, 1080.0]] {
        let (_, hits, _) = paint(Default::default(), |c| {
            c.title_artwork = Some(ui::IconRef {
                color_mask: None,
                page: 18,
                uv: [0, 0, 400, 80],
                glint: false,
            });
            draw(c, &view, size, &|_| None).unwrap();
        });
        let resume = hits
            .iter()
            .find(|(action, _)| *action == MenuAction::PauseResume)
            .unwrap()
            .1;
        assert!((resume.min().x() - ((size[0] - 780.0) * 0.5 + 24.0)).abs() < 0.001);
        assert!((resume.min().y() - ((size[1] - 496.0) * 0.5 + 296.0)).abs() < 0.001);
        assert!((resume.height() - 48.0).abs() < 0.001);
    }
}

#[test]
fn pause_shows_gameplay_input_mode_toggle_when_touch_available() {
    for (mode, expected_label) in [
        (semantic_input::InputMode::Touch, "Input: Touch"),
        (
            semantic_input::InputMode::KeyboardMouse,
            "Input: Keyboard & Mouse",
        ),
    ] {
        let mut view = MenuView::new(true, "Fixture".into());
        view.touch_available = true;
        view.gameplay_input_mode = mode;
        let (_, hits, nodes) = paint(Default::default(), |c| {
            draw(c, &view, [1280.0, 720.0], &|_| None).unwrap();
        });
        let toggle = hits
            .iter()
            .find(|(action, _)| *action == MenuAction::ToggleGameplayInputMode);
        assert!(
            toggle.is_some(),
            "Input mode toggle must be present when touch_available is true"
        );
        let has_label = nodes.iter().any(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. } => {
                let text: String = layout.glyphs().iter().map(|g| g.codepoint).collect();
                text == expected_label
            }
            _ => false,
        });
        assert!(
            has_label,
            "Button label must be {expected_label} for {mode:?}"
        );
    }
}

#[test]
fn pause_hides_gameplay_input_mode_toggle_when_touch_unavailable() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.touch_available = false;
    let (_, hits, _) = paint(Default::default(), |c| {
        draw(c, &view, [1280.0, 720.0], &|_| None).unwrap();
    });
    let toggle = hits
        .iter()
        .find(|(action, _)| *action == MenuAction::ToggleGameplayInputMode);
    assert!(
        toggle.is_none(),
        "Input mode toggle must be absent when touch_available is false"
    );
}

#[test]
fn pause_compact_layout_includes_input_toggle_when_touch_available() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.touch_available = true;
    view.gameplay_input_mode = semantic_input::InputMode::Touch;
    let (_, hits, _) = paint(Default::default(), |c| {
        draw(c, &view, [480.0, 320.0], &|_| None).unwrap();
    });
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::ToggleGameplayInputMode),
        "Compact pause menu must include input toggle when touch is available"
    );
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::Navigate(MenuScreen::DressingRoom)),
        "Compact pause menu must also preserve Dressing Room"
    );
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::PauseDisconnect),
        "Compact pause menu must preserve Save & Quit"
    );
}
