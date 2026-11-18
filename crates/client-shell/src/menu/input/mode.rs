//! The input source that controls menu focus and hints.

#[derive(Clone, Copy, Debug, Default)]
pub(in crate::menu) enum MenuInputMode {
    #[default]
    Mouse,
    Touch,
    Keyboard,
    Gamepad,
}

impl MenuInputMode {
    /// Whether mouse input owns the menu.
    pub(in crate::menu) fn mouse(self) -> bool {
        matches!(self, Self::Mouse)
    }

    /// Whether keyboard or gamepad navigation owns the menu.
    pub(in crate::menu) fn navigation(self) -> bool {
        matches!(self, Self::Keyboard | Self::Gamepad)
    }

    /// Whether gamepad input owns the menu.
    pub(in crate::menu) fn gamepad(self) -> bool {
        matches!(self, Self::Gamepad)
    }

    /// Whether touch input owns the menu.
    pub(in crate::menu) fn touch(self) -> bool {
        matches!(self, Self::Touch)
    }
}
