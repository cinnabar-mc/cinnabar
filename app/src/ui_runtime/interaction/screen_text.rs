//! Keyboard editing in an open screen's search or anvil name field: caret and deletion keys,
//! Shift selection, and the Control/Command select-all, copy, cut and paste shortcuts.

use bevy::prelude::KeyCode;
use client_shell::{menu::MenuClipboard, physical_input::keyboard_key};
use client_ui::ui_runtime::{PlatformClipboard, UiRuntime, interaction::is_chat_edit_shortcut};
use semantic_input::KeyboardKey;

/// Applies `key` to the focused search or anvil field, with `held` the keys held with it.
/// `false` leaves the press to the screen's other text handling: typed characters, Escape
/// and an editable book's keys.
pub(super) fn edit_field_key(
    runtime: &mut UiRuntime,
    key: KeyCode,
    held: &[KeyboardKey],
    clipboard: Option<&mut MenuClipboard>,
) -> bool {
    let shortcut = is_chat_edit_shortcut(held);
    let selecting =
        held.contains(&KeyboardKey::ShiftLeft) || held.contains(&KeyboardKey::ShiftRight);
    let state = runtime.screen_state_mut();
    if state.field_caret().is_none() {
        return false;
    }
    if state.edit_text_key(keyboard_key(key), selecting, shortcut) {
        return true;
    }
    if !shortcut {
        return false;
    }
    match key {
        KeyCode::KeyA => {
            state.select_all_text();
        }
        KeyCode::KeyC | KeyCode::KeyX => {
            let text = if key == KeyCode::KeyX {
                state.cut_text()
            } else {
                state.selected_text().map(str::to_owned)
            };
            if let Some(text) = text {
                match clipboard {
                    Some(clipboard) => clipboard.write_text(text),
                    None => {
                        let _ = PlatformClipboard.write_text(text);
                    }
                }
            }
        }
        KeyCode::KeyV => {
            match clipboard {
                Some(clipboard) => state.paste_text(clipboard),
                None => state.paste_text(&mut PlatformClipboard),
            };
        }
        _ => return false,
    }
    true
}
