use mod_ui::PanelUiOwner;
use ui::mod_panel::{Control, Event};

use super::mod_panel::{frame, point};
use crate::test_support::mini_engine_presentation;
use crate::ui_runtime::presentation::UiPresentationRuntime;
use mod_ui::test_support::sample_panel as panel;

fn opened() -> UiPresentationRuntime {
    let mut presentation = mini_engine_presentation();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel()))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    frame(&mut presentation, [1280, 720]);
    presentation
}

/// A press that opens a native field editor is still reported as a press on its control.
#[test]
fn native_editor_presses_report_their_control() {
    let mut presentation = opened();
    let number = point(&presentation, "mod.edit:1", 0.5);
    presentation
        .mod_ui_mut()
        .mod_panel_events(number, true, true);
    let (_, press) = presentation.mod_ui().mod_panel_press().unwrap();
    assert_eq!(press.id, "strength");
    assert!(press.held);
}

#[test]
fn pointer_cancellation_preserves_number_edits() {
    let mut presentation = opened();
    let number = point(&presentation, "mod.edit:1", 0.5);
    presentation
        .mod_ui_mut()
        .mod_panel_events(number, true, true);
    presentation
        .mod_ui_mut()
        .mod_panel_key("Digit4", Some("42"));
    presentation.mod_ui_mut().cancel_mod_panel_pointer_input();
    assert!(presentation.mod_ui().mod_panel_open() && presentation.mod_ui().mod_panel_editing());
    assert_eq!(
        presentation.mod_ui_mut().mod_panel_key("Enter", None),
        vec![Event {
            id: "strength".into(),
            value: 42.0
        }]
    );
}

#[test]
fn pointer_cancellation_prevents_a_previous_slider_drag_from_resuming() {
    let mut presentation = opened();
    let initial = point(&presentation, "mod.control:1", 0.25);
    assert!(
        !presentation
            .mod_ui_mut()
            .mod_panel_events(initial, true, true)
            .is_empty()
    );
    presentation.mod_ui_mut().cancel_mod_panel_pointer_input();
    let later = point(&presentation, "mod.control:1", 0.9);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(later, false, true)
            .is_empty()
    );
    assert!(presentation.mod_ui().mod_panel_open());
}

/// A press outside the open value editor dismisses it and discards the draft.
#[test]
fn value_editor_dismisses_on_an_outside_press_without_changing_value() {
    let mut presentation = opened();
    let number = point(&presentation, "mod.edit:1", 0.5);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(number, true, true)
            .is_empty()
    );
    presentation
        .mod_ui_mut()
        .mod_panel_key("Digit4", Some("42"));
    // The editor's dismissing layer is hit-tested once a frame has laid it out.
    frame(&mut presentation, [1280, 720]);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events([0., 0.], true, true)
            .is_empty()
    );
    assert!(!presentation.mod_ui().mod_panel_editing());
    frame(&mut presentation, [1280, 720]);
    let number = point(&presentation, "mod.edit:1", 0.5);
    presentation
        .mod_ui_mut()
        .mod_panel_events(number, true, true);
    assert_eq!(
        presentation.mod_ui_mut().mod_panel_key("Enter", None),
        vec![Event {
            id: "strength".into(),
            value: 35.
        }]
    );
}

#[test]
fn number_input_commits_only_valid_bounded_values_and_respects_step() {
    let mut presentation = opened();
    let mut spec = panel();
    if let Control::Slider { step, .. } = &mut spec.controls[1] {
        *step = 0.5;
    }
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&spec))
        .unwrap();
    frame(&mut presentation, [1280, 720]);
    let value = point(&presentation, "mod.edit:1", 0.5);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(value, true, true)
            .is_empty()
    );
    presentation
        .mod_ui_mut()
        .mod_panel_key("Digit9", Some("999"));
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_key("Enter", None)
            .is_empty()
    );
    assert!(presentation.mod_ui().mod_panel_editing());
    frame(&mut presentation, [1280, 720]);
    presentation.mod_ui_mut().mod_panel_key("SelectAll", None);
    presentation
        .mod_ui_mut()
        .mod_panel_key("Digit4", Some("42.3"));
    assert_eq!(
        presentation.mod_ui_mut().mod_panel_key("Enter", None),
        vec![Event {
            id: "strength".into(),
            value: 42.5
        }]
    );
    assert!(!presentation.mod_ui().mod_panel_editing());
    frame(&mut presentation, [1280, 720]);
    let value = point(&presentation, "mod.edit:1", 0.5);
    presentation
        .mod_ui_mut()
        .mod_panel_events(value, true, true);
    presentation.mod_ui_mut().mod_panel_key("Backspace", None);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_key("Enter", None)
            .is_empty()
    );
    assert!(presentation.mod_ui().mod_panel_editing());
    presentation.mod_ui_mut().mod_panel_key("Escape", None);
    assert!(presentation.mod_ui().mod_panel_open());
}

#[test]
fn number_input_handles_physical_digits_caret_deletion_and_focus_cancellation() {
    let mut presentation = opened();
    let value = point(&presentation, "mod.edit:1", 0.5);
    presentation
        .mod_ui_mut()
        .mod_panel_events(value, true, true);
    presentation.mod_ui_mut().mod_panel_key("Digit4", None);
    presentation.mod_ui_mut().mod_panel_key("Numpad2", None);
    presentation.mod_ui_mut().mod_panel_key("Home", None);
    presentation.mod_ui_mut().mod_panel_key("Delete", None);
    assert_eq!(
        presentation.mod_ui_mut().mod_panel_key("NumpadEnter", None),
        vec![Event {
            id: "strength".into(),
            value: 2.
        }]
    );
    frame(&mut presentation, [1280, 720]);
    let value = point(&presentation, "mod.edit:1", 0.5);
    presentation
        .mod_ui_mut()
        .mod_panel_events(value, true, true);
    presentation.mod_ui_mut().mod_panel_key("Digit8", None);
    presentation.mod_ui_mut().set_mod_panel_open(false);
    assert!(!presentation.mod_ui().mod_panel_editing());
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_key("Enter", None)
            .is_empty()
    );
    presentation.mod_ui_mut().set_mod_panel_open(true);
    frame(&mut presentation, [1280, 720]);
    assert!(!presentation.mod_ui().mod_panel_editing());
}

#[test]
fn guest_key_capture_prevents_opening_a_competing_text_editor() {
    let mut presentation = opened();
    let mut spec = panel();
    spec.capture_key = true;
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&spec))
        .unwrap();
    frame(&mut presentation, [1280, 720]);
    let value = point(&presentation, "mod.edit:1", 0.5);
    presentation
        .mod_ui_mut()
        .mod_panel_events(value, true, true);
    assert!(!presentation.mod_ui().mod_panel_editing());
}

#[test]
fn number_open_and_enter_preserves_sub_cent_precision() {
    let mut presentation = opened();
    let mut spec = panel();
    if let Control::Slider {
        value, max, step, ..
    } = &mut spec.controls[1]
    {
        *value = 0.123;
        *max = 1.;
        *step = 0.001;
    }
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&spec))
        .unwrap();
    frame(&mut presentation, [1280, 720]);
    let value = point(&presentation, "mod.edit:1", 0.5);
    presentation
        .mod_ui_mut()
        .mod_panel_events(value, true, true);
    let events = presentation.mod_ui_mut().mod_panel_key("Enter", None);
    assert_eq!(events.len(), 1);
    assert!((events[0].value - 0.123).abs() < 0.000001);
}

#[test]
fn same_shape_guest_updates_keep_draft_but_shape_changes_cancel_it() {
    let mut presentation = opened();
    let value = point(&presentation, "mod.edit:1", 0.5);
    presentation
        .mod_ui_mut()
        .mod_panel_events(value, true, true);
    presentation
        .mod_ui_mut()
        .mod_panel_key("Digit4", Some("44"));
    let mut spec = panel();
    if let Control::Slider { value, .. } = &mut spec.controls[1] {
        *value = 60.;
    }
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&spec))
        .unwrap();
    frame(&mut presentation, [1280, 720]);
    assert!(presentation.mod_ui().mod_panel_editing());
    assert_eq!(
        presentation.mod_ui_mut().mod_panel_key("Enter", None),
        vec![Event {
            id: "strength".into(),
            value: 44.
        }]
    );
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_key("Enter", None)
            .is_empty()
    );
    frame(&mut presentation, [1280, 720]);
    let value = point(&presentation, "mod.edit:1", 0.5);
    presentation
        .mod_ui_mut()
        .mod_panel_events(value, true, true);
    if let Control::Slider { max, .. } = &mut spec.controls[1] {
        *max = 90.;
    }
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&spec))
        .unwrap();
    assert!(!presentation.mod_ui().mod_panel_editing());
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_key("Enter", None)
            .is_empty()
    );
}
