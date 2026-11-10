use mod_ui::PanelUiOwner;
use std::sync::Arc;

use render_model::UiRenderInput;
use ui::{
    DpiScale,
    mod_panel::{Control, Event},
};

use super::super::super::snapshot;
use crate::test_support::mini_engine_presentation;
use crate::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

use mod_ui::test_support::sample_panel as panel;

pub(super) fn frame(presentation: &mut UiPresentationRuntime, size: [u32; 2]) -> UiRenderInput {
    presentation
        .build(
            &player_state::PlayerState::new(1),
            &UiRuntime::new(1),
            0,
            size,
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

pub(super) fn point(presentation: &UiPresentationRuntime, action: &str, fraction: f64) -> [f32; 2] {
    let state = presentation.mod_ui().mod_panel_state().unwrap();
    let frame = state.frame.unwrap();
    let region = frame
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some(action))
        .unwrap_or_else(|| panic!("missing rendered action {action}; hits={:?}", frame.hits));
    [
        frame.origin[0] + (region.rect.x + region.rect.w * fraction) as f32 * frame.scale,
        frame.origin[1] + (region.rect.y + region.rect.h * 0.5) as f32 * frame.scale,
    ]
}

#[test]
fn absent_or_closed_panel_does_not_change_the_frame() {
    let mut presentation = mini_engine_presentation();
    let base = frame(&mut presentation, [1280, 720]);
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel()))
        .unwrap();
    assert!(base == frame(&mut presentation, [1280, 720]));
    presentation.mod_ui_mut().set_mod_panel_open(true);
    let open = frame(&mut presentation, [1280, 720]);
    assert!(snapshot::rasterize(&base) != snapshot::rasterize(&open));
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), None)
        .unwrap();
    assert!(!presentation.mod_ui().mod_panel_open());
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events([500.0, 400.0], true, true)
            .is_empty()
    );
    let revoked = frame(&mut presentation, [1280, 720]);
    assert!(snapshot::rasterize(&base) == snapshot::rasterize(&revoked));
    assert!(base.vertices == revoked.vertices);
    assert!(base.indices == revoked.indices);
}

#[test]
fn actual_engine_geometry_routes_all_control_types() {
    let mut presentation = mini_engine_presentation();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel()))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events([500.0, 400.0], true, true)
            .is_empty()
    );
    frame(&mut presentation, [1280, 720]);
    let toggle = point(&presentation, "mod.control:0", 0.5);
    assert_eq!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(toggle, true, true),
        vec![Event {
            id: "enabled".into(),
            value: 1.0
        }]
    );
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(toggle, false, true)
            .is_empty()
    );
    let choice = point(&presentation, "mod.control:2", 0.5);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(choice, true, true)
            .is_empty()
    );
    frame(&mut presentation, [1280, 720]);
    let option = point(&presentation, "mod.option:1", 0.5);
    assert_eq!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(option, true, true),
        vec![Event {
            id: "mode".into(),
            value: 1.0
        }]
    );
    frame(&mut presentation, [1280, 720]);
    let button = point(&presentation, "mod.control:3", 0.5);
    assert_eq!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(button, true, true),
        vec![Event {
            id: "binding".into(),
            value: 1.0
        }]
    );
    let slider = point(&presentation, "mod.control:1", 0.62);
    assert_eq!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(slider, true, true),
        vec![Event {
            id: "strength".into(),
            value: 62.0
        }]
    );
    let beyond = [20_000.0, slider[1]];
    assert_eq!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(beyond, false, true),
        vec![Event {
            id: "strength".into(),
            value: 100.0
        }]
    );
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(beyond, false, true)
            .is_empty()
    );
    presentation
        .mod_ui_mut()
        .mod_panel_events(beyond, false, false);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(slider, false, true)
            .is_empty()
    );
}

#[test]
fn value_updates_reuse_catalog_and_unchanged_frame_reuses_layout() {
    let mut presentation = mini_engine_presentation();
    let mut settings = panel();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&settings))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    let original_frame = frame(&mut presentation, [1280, 720]);
    let original = Arc::clone(
        presentation
            .mod_ui()
            .mod_panel_state()
            .unwrap()
            .catalog
            .unwrap(),
    );
    let before = presentation.mod_ui().mod_panel_state().unwrap().passes;
    frame(&mut presentation, [1280, 720]);
    assert_eq!(
        before,
        presentation.mod_ui().mod_panel_state().unwrap().passes
    );
    if let Control::Slider { value, .. } = &mut settings.controls[1] {
        *value = 64.0;
    }
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&settings))
        .unwrap();
    let updated_frame = frame(&mut presentation, [1280, 720]);
    assert!(original_frame.vertices != updated_frame.vertices);
    assert!(Arc::ptr_eq(
        &original,
        presentation
            .mod_ui()
            .mod_panel_state()
            .unwrap()
            .catalog
            .unwrap()
    ));
    assert!(presentation.mod_ui().mod_panel_open());
    presentation.mod_ui_mut().set_mod_panel_open(false);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events([100.0, 100.0], true, true)
            .is_empty()
    );
}

#[test]
fn desktop_panel_geometry_stays_stable_across_game_gui_preferences_and_dpi() {
    let mut presentation = mini_engine_presentation();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel()))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    presentation.set_gui_scale_preference(Some(2));
    frame(&mut presentation, [1280, 720]);
    let expected = point(&presentation, "mod.control:1", 0.5);
    for preference in [1, 3, 4] {
        presentation.set_gui_scale_preference(Some(preference));
        frame(&mut presentation, [1280, 720]);
        assert_eq!(point(&presentation, "mod.control:1", 0.5), expected);
    }
    presentation
        .build(
            &player_state::PlayerState::new(1),
            &UiRuntime::new(1),
            0,
            [1600, 900],
            DpiScale::new(1.25).unwrap(),
        )
        .unwrap();
    assert_eq!(point(&presentation, "mod.control:1", 0.5), expected);
    assert!(
        !presentation
            .mod_ui_mut()
            .mod_panel_events(expected, true, true)
            .is_empty()
    );
}

#[test]
fn standard_keybind_displays_assigned_key_and_capture_state_without_rebuilding() {
    let mut presentation = mini_engine_presentation();
    let mut settings = panel();
    settings.controls = vec![Control::Keybind {
        id: "binding".into(),
        label: "Keybind".into(),
        key: "ControlRight".into(),
        capturing: false,
    }];
    settings.surface = mod_ui::test_support::sample_surface(&settings.controls);
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&settings))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    let assigned = frame(&mut presentation, [1280, 720]);
    let catalog = presentation
        .mod_ui()
        .mod_panel_state()
        .unwrap()
        .catalog
        .unwrap()
        .clone();
    let keycap = point(&presentation, "mod.control:0", 0.5);
    assert_eq!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(keycap, true, true),
        vec![Event {
            id: "binding".into(),
            value: 1.
        }]
    );
    if let Control::Keybind { capturing, .. } = &mut settings.controls[0] {
        *capturing = true;
    }
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&settings))
        .unwrap();
    let capture = frame(&mut presentation, [1280, 720]);
    assert_ne!(
        assigned.vertices, capture.vertices,
        "keycap must render its changing bound value"
    );
    assert!(Arc::ptr_eq(
        &catalog,
        presentation
            .mod_ui()
            .mod_panel_state()
            .unwrap()
            .catalog
            .unwrap()
    ));
    assert_eq!(point(&presentation, "mod.control:0", 0.5), keycap);
}
