//! USB HID keyboard usages, as the Game Controller framework reports keys, to Bevy keys.

use std::{collections::HashMap, sync::OnceLock};

use bevy::input::keyboard::{Key, KeyCode, NativeKey};

use crate::physical_input::{DESKTOP_KEYS, keyboard_usage};

/// The key with this HID usage among the keys desktop input translates.
pub(super) fn key_code(usage: u16) -> Option<KeyCode> {
    static KEYS: OnceLock<HashMap<u16, KeyCode>> = OnceLock::new();
    KEYS.get_or_init(|| {
        DESKTOP_KEYS
            .iter()
            .filter_map(|(key, _)| Some((keyboard_usage(*key)?, *key)))
            .collect()
    })
    .get(&usage)
    .copied()
}

/// The logical key for keys that name an action rather than type a character. Character keys
/// stay unidentified: the platform's text input reports what they type.
pub(super) fn logical_key(key: KeyCode) -> Key {
    match key {
        KeyCode::Escape => Key::Escape,
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Tab => Key::Tab,
        KeyCode::ArrowUp => Key::ArrowUp,
        KeyCode::ArrowDown => Key::ArrowDown,
        KeyCode::ArrowLeft => Key::ArrowLeft,
        KeyCode::ArrowRight => Key::ArrowRight,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => Key::Shift,
        KeyCode::ControlLeft | KeyCode::ControlRight => Key::Control,
        KeyCode::AltLeft | KeyCode::AltRight => Key::Alt,
        KeyCode::SuperLeft | KeyCode::SuperRight => Key::Super,
        KeyCode::F1 => Key::F1,
        KeyCode::F2 => Key::F2,
        KeyCode::F3 => Key::F3,
        KeyCode::F5 => Key::F5,
        _ => Key::Unidentified(NativeKey::Unidentified),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_translated_desktop_key_round_trips_through_its_usage() {
        for (key, _) in DESKTOP_KEYS {
            if let Some(usage) = keyboard_usage(*key) {
                assert_eq!(key_code(usage), Some(*key), "usage {usage:#04x}");
            }
        }
        // `GCKeyCodeKeyA`, `GCKeyCodeSpacebar` and `GCKeyCodeEscape`.
        assert_eq!(key_code(0x04), Some(KeyCode::KeyA));
        assert_eq!(key_code(0x2c), Some(KeyCode::Space));
        assert_eq!(key_code(0x29), Some(KeyCode::Escape));
        assert_eq!(key_code(0), None);
    }

    #[test]
    fn action_keys_are_named_and_character_keys_are_not() {
        assert_eq!(logical_key(KeyCode::Escape), Key::Escape);
        assert_eq!(logical_key(KeyCode::NumpadEnter), Key::Enter);
        assert_eq!(
            logical_key(KeyCode::KeyW),
            Key::Unidentified(NativeKey::Unidentified)
        );
    }
}
