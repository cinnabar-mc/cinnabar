//! What a selected edit box takes from the clipboard and from held modifiers.

use std::sync::Arc;

use super::*;
use crate::ui_runtime::presentation::forms::{
    ServerUiPack, pack_harness, tests::mini_engine_presentation,
};

/// A clipboard that holds `text` and honors the bound as the platform's does.
struct Clipboard {
    text: String,
    reads: Vec<usize>,
}

impl Clipboard {
    fn holding(text: String) -> Self {
        Self {
            text,
            reads: Vec::new(),
        }
    }
}

impl ChatClipboard for Clipboard {
    type Error = ();

    fn read_text_bounded(&mut self, maximum_bytes: usize) -> Result<Option<Arc<str>>, ()> {
        self.reads.push(maximum_bytes);
        if self.text.len() > maximum_bytes {
            return Err(());
        }
        Ok(Some(Arc::from(self.text.as_str())))
    }
}

/// A server form drawn with one edit box of `max_length` characters, selected.
fn selected_edit_box(max_length: i64) -> (UiRuntime, EngineFrame) {
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut presentation = mini_engine_presentation();
    let form_button = format!(
        r#"{{
            "namespace": "server_form",
            "form_button": {{
                "type": "edit_box", "size": [200, 30], "text_control": "display",
                "max_length": {max_length},
                "button_mappings": [{{ "from_button_id": "button.menu_select",
                    "to_button_id": "button.text_edit_box_selected",
                    "mapping_type": "pressed", "handle_select": true,
                    "handle_deselect": false }}],
                "controls": [{{ "display": {{ "type": "label", "text": "",
                    "size": [200, 30] }} }}]
            }}
        }}"#
    );
    presentation.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![(
            "ui/server_form.json".into(),
            form_button.into_bytes(),
        )]],
        ..Default::default()
    });
    let mut runtime = pack_harness::action_form(&mut player_runtime, "Shop", &["A"]);
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
    let edit = frame
        .hits
        .iter()
        .find(|hit| hit.kind == HitKind::EditBox)
        .unwrap()
        .key
        .clone();
    let engine = runtime.server_forms_mut().engine_mut();
    engine.view.focused = Some(edit);
    engine.dispatcher.button(
        &frame.hits,
        &mut engine.view,
        EngineButton {
            id: SELECT,
            down: true,
            point: None,
            mode: InputMode::Gamepad,
            now: 0.0,
        },
    );
    assert!(editing(runtime.server_forms().engine(), &frame));
    (runtime, frame)
}

/// The text a Ctrl+V leaves in the box, read from `clipboard`.
fn pasted(max_length: i64, clipboard: &mut Clipboard) -> Option<String> {
    let (mut runtime, frame) = selected_edit_box(max_length);
    let engine = runtime.server_forms_mut().engine_mut();
    keyboard(
        engine,
        &frame,
        KeyboardKey::KeyV,
        None,
        true,
        0.0,
        clipboard,
    )
    .into_iter()
    .find_map(|event| match event {
        ScreenEvent::TextEdit { text, .. } => Some(text),
        _ => None,
    })
}

/// The longest command a command block's text box holds.
const COMMAND_CAPACITY: i64 = 32_500;

#[test]
fn a_paste_within_the_boxs_capacity_reaches_it() {
    let five_kilobytes = "a".repeat(5 * 1024);
    let mut clipboard = Clipboard::holding(five_kilobytes.clone());
    assert_eq!(
        pasted(COMMAND_CAPACITY, &mut clipboard),
        Some(five_kilobytes)
    );

    // The bound is in bytes and the box's capacity in characters.
    let widest = "\u{1F980}".repeat(COMMAND_CAPACITY as usize);
    let mut clipboard = Clipboard::holding(widest.clone());
    assert_eq!(pasted(COMMAND_CAPACITY, &mut clipboard), Some(widest));
}

#[test]
fn a_paste_past_the_boxs_capacity_changes_nothing() {
    // Too many characters, in few enough bytes to be read.
    let mut clipboard = Clipboard::holding("a".repeat(6));
    assert_eq!(pasted(5, &mut clipboard), None);
    assert_eq!(clipboard.reads, [20]);

    // Too many bytes to hold in any encoding, so the read itself is refused.
    let mut clipboard = Clipboard::holding("a".repeat(21));
    assert_eq!(pasted(5, &mut clipboard), None);
    assert_eq!(clipboard.reads, [20]);

    // A box without a capacity reads nothing that takes any room.
    let mut clipboard = Clipboard::holding("a".into());
    assert_eq!(pasted(-1, &mut clipboard), None);
    assert_eq!(clipboard.reads, [0]);
}

/// What pressing `key` producing `text` types with `held` down.
fn typed_with(held: &[KeyboardKey], key: KeyboardKey, text: &str) -> Option<String> {
    let (mut runtime, frame) = selected_edit_box(10);
    let engine = runtime.server_forms_mut().engine_mut();
    let input = EngineInput {
        press_shortcuts: Vec::new(),
        touch: false,
        cursor: None,
        keys: held,
        pointer: PointerButtons::default(),
        pointer_edges: Vec::new(),
        wheel: Vec::new(),
        typed: vec![(key, Some(text.to_owned()), false)],
        menu_cancel: None,
        now: 0.0,
        animator: None,
    };
    dispatch(engine, &frame, input)
        .events
        .into_iter()
        .find_map(|event| match event {
            ScreenEvent::TextEdit { text, .. } => Some(text),
            _ => None,
        })
}

/// AltGr reaches some layouts' characters as Ctrl+Alt, which isn't an edit shortcut.
#[test]
fn control_with_alt_still_types_a_layouts_characters() {
    assert_eq!(
        typed_with(
            &[KeyboardKey::ControlLeft, KeyboardKey::AltRight],
            KeyboardKey::KeyQ,
            "@"
        ),
        Some("@".to_owned())
    );
    assert_eq!(
        typed_with(&[KeyboardKey::ControlLeft], KeyboardKey::KeyQ, "q"),
        None
    );
}
