//! Desktop key and wheel identities shared by UI and gameplay without a windowing backend.

/// A physical desktop key, named by its US-layout position.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum KeyboardKey {
    KeyA,
    KeyD,
    KeyB,
    KeyC,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyT,
    KeyU,
    KeyV,
    KeyX,
    KeyY,
    KeyZ,
    KeyS,
    KeyW,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Digit0,
    Backspace,
    Enter,
    Escape,
    Tab,
    Space,
    Minus,
    Equal,
    BracketLeft,
    BracketRight,
    Backslash,
    Semicolon,
    Quote,
    Backquote,
    Comma,
    Period,
    Slash,
    Insert,
    Home,
    PageUp,
    Delete,
    End,
    PageDown,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    ArrowRight,
    ArrowLeft,
    ArrowDown,
    ArrowUp,
    CapsLock,
    PrintScreen,
    ScrollLock,
    Pause,
    NumLock,
    NumpadDivide,
    NumpadMultiply,
    NumpadSubtract,
    NumpadAdd,
    NumpadEnter,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    Numpad0,
    NumpadDecimal,
    IntlBackslash,
    ContextMenu,
    NumpadEqual,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
    NumpadComma,
    ControlLeft,
    ShiftLeft,
    AltLeft,
    SuperLeft,
    ControlRight,
    ShiftRight,
    AltRight,
    SuperRight,
    /// A key without a USB HID usage; text it produces still reaches edit boxes. Stays last, so
    /// its discriminant counts the keyed variants.
    Unidentified,
}

/// Desktop keys and their USB HID Keyboard/Keypad page (0x07) usages.
/// Reference: https://www.usb.org/sites/default/files/hut1_3_0.pdf, section 10.
pub const KEYBOARD_USAGES: &[(KeyboardKey, u16)] = &[
    (KeyboardKey::KeyA, 0x04),
    (KeyboardKey::KeyD, 0x07),
    (KeyboardKey::KeyB, 0x05),
    (KeyboardKey::KeyC, 0x06),
    (KeyboardKey::KeyE, 0x08),
    (KeyboardKey::KeyF, 0x09),
    (KeyboardKey::KeyG, 0x0a),
    (KeyboardKey::KeyH, 0x0b),
    (KeyboardKey::KeyI, 0x0c),
    (KeyboardKey::KeyJ, 0x0d),
    (KeyboardKey::KeyK, 0x0e),
    (KeyboardKey::KeyL, 0x0f),
    (KeyboardKey::KeyM, 0x10),
    (KeyboardKey::KeyN, 0x11),
    (KeyboardKey::KeyO, 0x12),
    (KeyboardKey::KeyP, 0x13),
    (KeyboardKey::KeyQ, 0x14),
    (KeyboardKey::KeyR, 0x15),
    (KeyboardKey::KeyT, 0x17),
    (KeyboardKey::KeyU, 0x18),
    (KeyboardKey::KeyV, 0x19),
    (KeyboardKey::KeyX, 0x1b),
    (KeyboardKey::KeyY, 0x1c),
    (KeyboardKey::KeyZ, 0x1d),
    (KeyboardKey::KeyS, 0x16),
    (KeyboardKey::KeyW, 0x1a),
    (KeyboardKey::Digit1, 0x1e),
    (KeyboardKey::Digit2, 0x1f),
    (KeyboardKey::Digit3, 0x20),
    (KeyboardKey::Digit4, 0x21),
    (KeyboardKey::Digit5, 0x22),
    (KeyboardKey::Digit6, 0x23),
    (KeyboardKey::Digit7, 0x24),
    (KeyboardKey::Digit8, 0x25),
    (KeyboardKey::Digit9, 0x26),
    (KeyboardKey::Digit0, 0x27),
    (KeyboardKey::Backspace, 0x2a),
    (KeyboardKey::Enter, 0x28),
    (KeyboardKey::Escape, 0x29),
    (KeyboardKey::Tab, 0x2b),
    (KeyboardKey::Space, 0x2c),
    (KeyboardKey::Minus, 0x2d),
    (KeyboardKey::Equal, 0x2e),
    (KeyboardKey::BracketLeft, 0x2f),
    (KeyboardKey::BracketRight, 0x30),
    (KeyboardKey::Backslash, 0x31),
    (KeyboardKey::Semicolon, 0x33),
    (KeyboardKey::Quote, 0x34),
    (KeyboardKey::Backquote, 0x35),
    (KeyboardKey::Comma, 0x36),
    (KeyboardKey::Period, 0x37),
    (KeyboardKey::Slash, 0x38),
    (KeyboardKey::Insert, 0x49),
    (KeyboardKey::Home, 0x4a),
    (KeyboardKey::PageUp, 0x4b),
    (KeyboardKey::Delete, 0x4c),
    (KeyboardKey::End, 0x4d),
    (KeyboardKey::PageDown, 0x4e),
    (KeyboardKey::F1, 0x3a),
    (KeyboardKey::F2, 0x3b),
    (KeyboardKey::F3, 0x3c),
    (KeyboardKey::F4, 0x3d),
    (KeyboardKey::F5, 0x3e),
    (KeyboardKey::F6, 0x3f),
    (KeyboardKey::F7, 0x40),
    (KeyboardKey::F8, 0x41),
    (KeyboardKey::F9, 0x42),
    (KeyboardKey::F10, 0x43),
    (KeyboardKey::F11, 0x44),
    (KeyboardKey::F12, 0x45),
    // The UiFocused defaults bind these four HID usages; without them the
    // arrow keys are dead in every menu.
    (KeyboardKey::ArrowRight, 0x4f),
    (KeyboardKey::ArrowLeft, 0x50),
    (KeyboardKey::ArrowDown, 0x51),
    (KeyboardKey::ArrowUp, 0x52),
    (KeyboardKey::CapsLock, 0x39),
    (KeyboardKey::PrintScreen, 0x46),
    (KeyboardKey::ScrollLock, 0x47),
    (KeyboardKey::Pause, 0x48),
    (KeyboardKey::NumLock, 0x53),
    (KeyboardKey::NumpadDivide, 0x54),
    (KeyboardKey::NumpadMultiply, 0x55),
    (KeyboardKey::NumpadSubtract, 0x56),
    (KeyboardKey::NumpadAdd, 0x57),
    (KeyboardKey::NumpadEnter, 0x58),
    (KeyboardKey::Numpad1, 0x59),
    (KeyboardKey::Numpad2, 0x5a),
    (KeyboardKey::Numpad3, 0x5b),
    (KeyboardKey::Numpad4, 0x5c),
    (KeyboardKey::Numpad5, 0x5d),
    (KeyboardKey::Numpad6, 0x5e),
    (KeyboardKey::Numpad7, 0x5f),
    (KeyboardKey::Numpad8, 0x60),
    (KeyboardKey::Numpad9, 0x61),
    (KeyboardKey::Numpad0, 0x62),
    (KeyboardKey::NumpadDecimal, 0x63),
    (KeyboardKey::IntlBackslash, 0x64),
    (KeyboardKey::ContextMenu, 0x65),
    (KeyboardKey::NumpadEqual, 0x67),
    (KeyboardKey::F13, 0x68),
    (KeyboardKey::F14, 0x69),
    (KeyboardKey::F15, 0x6a),
    (KeyboardKey::F16, 0x6b),
    (KeyboardKey::F17, 0x6c),
    (KeyboardKey::F18, 0x6d),
    (KeyboardKey::F19, 0x6e),
    (KeyboardKey::F20, 0x6f),
    (KeyboardKey::F21, 0x70),
    (KeyboardKey::F22, 0x71),
    (KeyboardKey::F23, 0x72),
    (KeyboardKey::F24, 0x73),
    (KeyboardKey::NumpadComma, 0x85),
    (KeyboardKey::ControlLeft, 0xe0),
    (KeyboardKey::ShiftLeft, 0xe1),
    (KeyboardKey::AltLeft, 0xe2),
    (KeyboardKey::SuperLeft, 0xe3),
    (KeyboardKey::ControlRight, 0xe4),
    (KeyboardKey::ShiftRight, 0xe5),
    (KeyboardKey::AltRight, 0xe6),
    (KeyboardKey::SuperRight, 0xe7),
];

impl KeyboardKey {
    /// The USB HID usage settings and gameplay bindings persist for this key.
    pub fn usage(self) -> Option<u16> {
        KEYBOARD_USAGES
            .iter()
            .find_map(|(candidate, usage)| (*candidate == self).then_some(*usage))
    }

    /// Whether pressing the key enters a character into a focused text box, so a binding on it
    /// must not also act while the player types.
    pub const fn types_text(self) -> bool {
        match self {
            Self::KeyA
            | Self::KeyD
            | Self::KeyB
            | Self::KeyC
            | Self::KeyE
            | Self::KeyF
            | Self::KeyG
            | Self::KeyH
            | Self::KeyI
            | Self::KeyJ
            | Self::KeyK
            | Self::KeyL
            | Self::KeyM
            | Self::KeyN
            | Self::KeyO
            | Self::KeyP
            | Self::KeyQ
            | Self::KeyR
            | Self::KeyT
            | Self::KeyU
            | Self::KeyV
            | Self::KeyX
            | Self::KeyY
            | Self::KeyZ
            | Self::KeyS
            | Self::KeyW
            | Self::Digit1
            | Self::Digit2
            | Self::Digit3
            | Self::Digit4
            | Self::Digit5
            | Self::Digit6
            | Self::Digit7
            | Self::Digit8
            | Self::Digit9
            | Self::Digit0
            | Self::Space
            | Self::Minus
            | Self::Equal
            | Self::BracketLeft
            | Self::BracketRight
            | Self::Backslash
            | Self::Semicolon
            | Self::Quote
            | Self::Backquote
            | Self::Comma
            | Self::Period
            | Self::Slash
            | Self::NumpadDivide
            | Self::NumpadMultiply
            | Self::NumpadSubtract
            | Self::NumpadAdd
            | Self::Numpad1
            | Self::Numpad2
            | Self::Numpad3
            | Self::Numpad4
            | Self::Numpad5
            | Self::Numpad6
            | Self::Numpad7
            | Self::Numpad8
            | Self::Numpad9
            | Self::Numpad0
            | Self::NumpadDecimal
            | Self::IntlBackslash
            | Self::NumpadEqual
            | Self::NumpadComma
            | Self::Unidentified => true,
            Self::Backspace
            | Self::Enter
            | Self::Escape
            | Self::Tab
            | Self::Insert
            | Self::Home
            | Self::PageUp
            | Self::Delete
            | Self::End
            | Self::PageDown
            | Self::F1
            | Self::F2
            | Self::F3
            | Self::F4
            | Self::F5
            | Self::F6
            | Self::F7
            | Self::F8
            | Self::F9
            | Self::F10
            | Self::F11
            | Self::F12
            | Self::ArrowRight
            | Self::ArrowLeft
            | Self::ArrowDown
            | Self::ArrowUp
            | Self::CapsLock
            | Self::PrintScreen
            | Self::ScrollLock
            | Self::Pause
            | Self::NumLock
            | Self::NumpadEnter
            | Self::ContextMenu
            | Self::F13
            | Self::F14
            | Self::F15
            | Self::F16
            | Self::F17
            | Self::F18
            | Self::F19
            | Self::F20
            | Self::F21
            | Self::F22
            | Self::F23
            | Self::F24
            | Self::ControlLeft
            | Self::ShiftLeft
            | Self::AltLeft
            | Self::SuperLeft
            | Self::ControlRight
            | Self::ShiftRight
            | Self::AltRight
            | Self::SuperRight => false,
        }
    }
}

/// How a wheel delta is measured: whole lines or device pixels.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScrollUnit {
    Line,
    Pixel,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every keyed variant has exactly one HID usage, and no two keys share a usage.
    #[test]
    fn every_keyed_variant_has_one_distinct_usage() {
        let keyed = KeyboardKey::Unidentified as usize;
        let mut variants: Vec<_> = KEYBOARD_USAGES
            .iter()
            .map(|(key, _)| *key as usize)
            .collect();
        variants.sort_unstable();
        assert_eq!(variants, (0..keyed).collect::<Vec<_>>());
        let mut usages: Vec<_> = KEYBOARD_USAGES.iter().map(|(_, usage)| *usage).collect();
        usages.sort_unstable();
        usages.dedup();
        assert_eq!(usages.len(), keyed);
        assert_eq!(KeyboardKey::Unidentified.usage(), None);
    }

    /// Keys that enter a character count as typing; keys that enter none do not, so a binding on
    /// them keeps working inside a text box.
    #[test]
    fn only_keys_that_enter_a_character_type_text() {
        use KeyboardKey::*;
        for key in [
            KeyA,
            KeyM,
            KeyZ,
            Digit0,
            Digit5,
            Space,
            Minus,
            Slash,
            Comma,
            Period,
            Numpad0,
            NumpadAdd,
            NumpadDecimal,
            // A key the platform could not name may still enter text.
            Unidentified,
        ] {
            assert!(key.types_text(), "{key:?} should type text");
        }
        for key in [
            F1,
            F2,
            F12,
            ArrowLeft,
            ArrowUp,
            ControlLeft,
            ShiftRight,
            AltLeft,
            PrintScreen,
            CapsLock,
            Enter,
            Backspace,
            Tab,
            Escape,
        ] {
            assert!(!key.types_text(), "{key:?} should not type text");
        }
    }
}
