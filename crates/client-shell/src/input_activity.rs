//! Whether raw input reached this frame, for systems that only react to it.
use bevy::{
    ecs::{message::Messages, schedule::SystemSet},
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
/// these they would do nothing, so the set runs only under [`raw_input_activity`].
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RawInputReaders;

/// Whether this frame carries raw input: a key or mouse button changed state, a keyboard or
/// keyboard-focus message is buffered, a touch is down or just ended, or a gamepad is connected.
/// A missing input resource counts as input, so harnesses without one run every reader.
pub fn raw_input_activity(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    keyboard: Option<Res<Messages<KeyboardInput>>>,
    focus_lost: Option<Res<Messages<KeyboardFocusLost>>>,
    touches: Option<Res<Touches>>,
    gamepads: Query<(), With<Gamepad>>,
) -> bool {
    fn changed<T: Copy + Eq + std::hash::Hash + Send + Sync>(buttons: &ButtonInput<T>) -> bool {
        buttons.get_just_pressed().next().is_some() || buttons.get_just_released().next().is_some()
    }
    keys.is_none_or(|keys| changed(&keys))
        || mouse.is_none_or(|mouse| changed(&mouse))
        || keyboard.is_none_or(|messages| !messages.is_empty())
        || focus_lost.is_none_or(|messages| !messages.is_empty())
        || touches.is_none_or(|touches| {
            touches.iter().next().is_some()
                || touches.iter_just_released().next().is_some()
                || touches.iter_just_canceled().next().is_some()
        })
        || !gamepads.is_empty()
}

#[cfg(test)]
mod tests;
