//! Keyboard focus and a resting pointer agree on which control shows hover.

use super::*;

/// Arrow keys move the hover look away from a resting pointer, and it stays with keyboard focus
/// until the pointer moves, so a mouse left over a button cannot pull it back every frame.
#[test]
fn keyboard_focus_keeps_hover_until_the_pointer_moves() {
    // The vanilla button templates enable focus; the minimal test pack does not.
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        eprintln!(
            "skipping keyboard_focus_keeps_hover_until_the_pointer_moves: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = pack_harness::action_form(&mut player_runtime, "Shop", &["Buy", "Sell"]);
    presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation.form_engine_frame(identity).unwrap().clone();
    let button = |index| {
        frame
            .hits
            .iter()
            .find(|hit| hit.collection_index == Some(index))
            .unwrap()
            .clone()
    };
    let (first, second) = (button(0), button(1));
    let cursor_at = |offset: f64| {
        UiPoint::new(
            frame.origin[0] + (first.rect.x + offset) as f32 * frame.scale,
            frame.origin[1] + (first.rect.y + 2.0) as f32 * frame.scale,
        )
        .unwrap()
    };
    let mut step = |cursor: UiPoint, typed: Vec<(KeyboardKey, Option<String>, bool)>, now| {
        drive(
            &mut runtime,
            &frame,
            EngineInput {
                press_shortcuts: Vec::new(),
                touch: false,
                cursor: Some(cursor),
                keys: &[],
                pointer: PointerButtons::default(),
                pointer_edges: Vec::new(),
                wheel: Vec::new(),
                typed,
                menu_cancel: None,
                now,
                animator: None,
            },
        );
        let view = &runtime.server_forms().engine().view;
        (view.hovered.clone(), view.focused.clone())
    };
    let (first_key, second_key) = (Some(first.key.clone()), Some(second.key.clone()));
    assert_eq!(
        step(cursor_at(2.0), Vec::new(), 0.0),
        (first_key.clone(), first_key.clone()),
        "the pointer hovers and focuses the button under it"
    );
    let arrow = vec![(KeyboardKey::ArrowDown, None, false)];
    assert_eq!(
        step(cursor_at(2.0), arrow, 0.1).1,
        second_key,
        "the arrow key moves focus to the next button"
    );
    assert_eq!(
        step(cursor_at(2.0), Vec::new(), 0.2).0,
        second_key,
        "a resting pointer leaves hover with keyboard focus"
    );
    assert_eq!(
        step(cursor_at(8.0), Vec::new(), 0.3).0,
        first_key,
        "moving the pointer takes hover back"
    );
    assert_eq!(
        step(cursor_at(8.0), Vec::new(), 0.4).1,
        first_key,
        "focus follows the pointer onto a control"
    );
    let outside = UiPoint::new(1.0, 1.0).unwrap();
    assert!(
        !frame
            .hits
            .iter()
            .any(|hit| hit.contains(frame.to_virtual(outside)))
    );
    assert_eq!(
        step(outside, Vec::new(), 0.5).0,
        first_key,
        "leaving the controls keeps the last one the pointer was over"
    );
}

/// Hovering a disabled button preserves focus and keyboard activation on the enabled button.
#[test]
fn hovering_a_disabled_button_preserves_keyboard_activation() {
    let mut player = player_state::PlayerState::new(1);
    let runtime = pack_harness::action_form(&mut player, "Shop", &["Buy", "Sell"]);
    let mut presentation = mini_engine_presentation();
    presentation.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![(
            "ui/server_form.json".into(),
            br#"{
            "namespace": "server_form",
            "main_screen_content": {
                "type": "stack_panel", "size": [200, 60],
                "controls": [
                    {"active@server_form.focus_button": {"collection_index": 0}},
                    {"disabled@server_form.focus_button": {"collection_index": 1, "enabled": false}}
                ]
            },
            "focus_button": {
                "type": "button", "size": [200, 30], "focus_enabled": true,
                "button_mappings": [{
                    "from_button_id": "button.menu_ok",
                    "to_button_id": "button.form_button_click",
                    "mapping_type": "focused"
                }]
            }
        }"#
            .to_vec(),
        )]],
        ..Default::default()
    });
    presentation
        .build(
            &player,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation.form_engine_frame(identity).unwrap();
    let active = frame.hits.iter().find(|hit| hit.name == "active").unwrap();
    let disabled = frame
        .hits
        .iter()
        .find(|hit| hit.name == "disabled")
        .unwrap();
    assert!(active.takes_focus());
    assert!(!disabled.enabled);
    assert!(disabled.focus.as_ref().unwrap().enabled);
    let cursor_at = |hit: &HitRegion| {
        UiPoint::new(
            frame.origin[0] + (hit.rect.x + 1.0) as f32 * frame.scale,
            frame.origin[1] + (hit.rect.y + 1.0) as f32 * frame.scale,
        )
        .unwrap()
    };
    let step = |engine: &mut FormEngineState, cursor, typed| {
        dispatch(
            engine,
            frame,
            EngineInput {
                cursor,
                keys: &[],
                pointer: PointerButtons::default(),
                touch: false,
                pointer_edges: Vec::new(),
                wheel: Vec::new(),
                typed,
                press_shortcuts: Vec::new(),
                menu_cancel: None,
                now: 0.0,
                animator: None,
            },
        )
    };
    for key in [KeyboardKey::Enter, KeyboardKey::Space] {
        let mut engine = FormEngineState::default();
        step(&mut engine, Some(cursor_at(active)), Vec::new());
        assert_eq!(engine.view.focused.as_deref(), Some(active.key.as_str()));
        step(&mut engine, Some(cursor_at(disabled)), Vec::new());
        assert_eq!(engine.view.hovered.as_deref(), Some(disabled.key.as_str()));
        assert_eq!(engine.view.focused.as_deref(), Some(active.key.as_str()));
        let dispatched = step(&mut engine, None, vec![(key, None, false)]);
        assert!(
            dispatched.events.iter().any(|event| matches!(event,
                ScreenEvent::Button(button) if button.key == active.key
                    && button.id == "button.form_button_click" && activates(button, None)
            )),
            "{key:?}: {:?}",
            dispatched.events
        );
    }
}

/// A resting pointer only takes hover back after travelling past the slop on either axis, or
/// when it enters or leaves the window.
#[test]
fn the_pointer_counts_as_moved_only_past_the_slop_or_across_the_window_edge() {
    let rested = Some([10.0, 20.0]);
    assert!(!pointer_moved(rested, rested));
    let within = POINTER_MOVE_SLOP * 0.5;
    assert!(!pointer_moved(rested, Some([10.0 + within, 20.0 - within])));
    let past = POINTER_MOVE_SLOP * 2.0;
    assert!(pointer_moved(rested, Some([10.0 + past, 20.0])));
    assert!(pointer_moved(rested, Some([10.0, 20.0 - past])));
    assert!(!pointer_moved(None, None));
    assert!(pointer_moved(None, rested));
    assert!(pointer_moved(rested, None));
}
