//! Whether raw input reached this frame, for systems that only react to it.
use bevy::{
    ecs::{message::Messages, schedule::SystemSet, system::SystemParam},
    input::{
        ButtonInput,
        gamepad::Gamepad,
        keyboard::{KeyCode, KeyboardFocusLost, KeyboardInput},
        mouse::MouseButton,
        touch::Touches,
    },
    prelude::{Query, Res, With},
};

/// Systems that only act on raw input: a just-pressed or just-released key or mouse button, a
/// buffered keyboard or keyboard-focus message, a touch or a gamepad. On a frame with none of
/// these they would do nothing, so the set runs only under [`raw_input_activity`]. Readers of
/// routed or deferred input, such as a world tap's Use press after its finger lifts, stay out.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RawInputReaders;

/// The raw input a frame can carry, read without consuming any of it.
#[derive(SystemParam)]
pub struct RawInput<'w, 's> {
    keys: Option<Res<'w, ButtonInput<KeyCode>>>,
    mouse: Option<Res<'w, ButtonInput<MouseButton>>>,
    keyboard: Option<Res<'w, Messages<KeyboardInput>>>,
    focus_lost: Option<Res<'w, Messages<KeyboardFocusLost>>>,
    touches: Option<Res<'w, Touches>>,
    gamepads: Query<'w, 's, (), With<Gamepad>>,
}

impl RawInput<'_, '_> {
    /// Whether this frame carries raw input: a key or mouse button changed state, a keyboard or
    /// keyboard-focus message is buffered, a touch is down or just ended, or a gamepad is
    /// connected. A missing input resource counts as input, so harnesses without one run every
    /// reader.
    pub fn active(&self) -> bool {
        fn changed<T: Copy + Eq + std::hash::Hash + Send + Sync>(buttons: &ButtonInput<T>) -> bool {
            buttons.get_just_pressed().next().is_some()
                || buttons.get_just_released().next().is_some()
        }
        self.keys.as_deref().is_none_or(changed)
            || self.mouse.as_deref().is_none_or(changed)
            || self
                .keyboard
                .as_deref()
                .is_none_or(|messages| !messages.is_empty())
            || self
                .focus_lost
                .as_deref()
                .is_none_or(|messages| !messages.is_empty())
            || self.touches.as_deref().is_none_or(|touches| {
                touches.iter().next().is_some()
                    || touches.iter_just_released().next().is_some()
                    || touches.iter_just_canceled().next().is_some()
            })
            || !self.gamepads.is_empty()
    }
}

/// Runs [`RawInputReaders`] only on a frame that carries raw input; see [`RawInput::active`].
pub fn raw_input_activity(input: RawInput) -> bool {
    input.active()
}

#[cfg(test)]
mod tests;
