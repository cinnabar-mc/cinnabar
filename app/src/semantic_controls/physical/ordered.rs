//! Translates one backend arrival stream without combining independently ordered readers.

use std::time::Duration;

use super::*;
use bevy::{
    ecs::message::MessageCursor,
    input::ButtonState,
    prelude::Messages,
    window::WindowEvent,
};
use client_interaction::hotbar::{WheelAccumulator, wheel_pixels};
use semantic_input::{DesktopEvent, DesktopEvents, PhysicalControl};

#[cfg(test)]
mod tests;

/// Drains the backend batch even while capture is unavailable; missing order fails closed.
pub(super) fn collect(
    inputs: &SemanticPhysicalInputs,
    messages: Option<&Messages<WindowEvent>>,
    cursor: &mut MessageCursor<WindowEvent>,
    wheel_enabled: bool,
    ordered_owns_wheel: bool,
    mut wheel: Option<&mut WheelAccumulator>,
    now: Duration,
) -> Option<(DesktopEvents, bool)> {
    if !wheel_enabled {
        // A disabled wheel or a screen owning input leaves no pending motion behind.
        if let Some(wheel) = wheel.as_deref_mut() {
            wheel.clear();
        }
    }
    let messages = messages?;
    let window = inputs.window.0;
    let mut batch = Vec::new();
    let mut output = DesktopEvents::default();
    let mut steps = 0_i32;
    let mut lost = false;
    let reserved = |name: &str| {
        inputs
            .reserved
            .as_ref()
            .is_some_and(|reserved| reserved.keys.iter().any(|key| key == name))
    };
    // Platform buttons carry their own event-time modifiers, in the order they were written. They
    // are matched against every event, before reservations filter the batch.
    let mut platform_buttons = inputs
        .platform_buttons
        .as_ref()
        .map(|buttons| buttons.0.as_slice())
        .unwrap_or_default()
        .iter()
        .peekable();
    for event in cursor.read(messages) {
        let platform_modifiers = match event {
            WindowEvent::MouseButtonInput(input) => platform_buttons
                .next_if(|(written, _)| written == input)
                .map(|(_, modifiers)| *modifiers),
            _ => None,
        };
        let relevant = match event {
            WindowEvent::KeyboardInput(event) => {
                event.window == window
                    && !event.repeat
                    && !reserved(&format!("{:?}", event.key_code))
            }
            WindowEvent::MouseButtonInput(event) => {
                event.window == window && !reserved(&format!("Mouse{:?}", event.button))
            }
            WindowEvent::MouseWheel(event) => {
                event.window == window
                    && !inputs
                        .reserved
                        .as_ref()
                        .is_some_and(|reserved| reserved.scroll)
            }
            WindowEvent::KeyboardFocusLost(_) => {
                lost = true;
                false
            }
            WindowEvent::WindowFocused(event) => {
                lost |= event.window == window && !event.focused;
                false
            }
            WindowEvent::WindowOccluded(event) => {
                lost |= event.window == window && event.occluded;
                false
            }
            _ => false,
        };
        if relevant {
            batch.push((event.clone(), platform_modifiers));
        }
    }
    let mut keys = (*inputs.keys).clone();
    for (event, _) in batch.iter().rev() {
        if let WindowEvent::KeyboardInput(event) = event {
            change_key(
                &mut keys,
                event.key_code,
                event.state != ButtonState::Pressed,
            );
        }
    }
    output.set_initial_modifiers(modifiers(&keys));
    for (event, platform_modifiers) in batch {
        let button = match event {
            WindowEvent::KeyboardInput(event) => {
                let pressed = event.state == ButtonState::Pressed;
                change_key(&mut keys, event.key_code, pressed);
                keyboard_usage(event.key_code)
                    .map(|code| (PhysicalControl::KeyboardUsage(code), pressed))
            }
            WindowEvent::MouseButtonInput(event) => mouse_button_code(event.button).map(|code| {
                (
                    PhysicalControl::MouseButton(code),
                    event.state == ButtonState::Pressed,
                )
            }),
            WindowEvent::MouseWheel(event) => {
                // Keep the wheel source gated here, before it becomes a generic HotbarNext or
                // HotbarPrevious action. Digits and bound buttons never take this branch.
                // Non-KeyboardMouse modes leave the accumulator for the raw hotbar path.
                let step = if let Some(wheel) = wheel.as_deref_mut() {
                    if wheel_enabled && ordered_owns_wheel {
                        wheel.advance(wheel_pixels(event.y, event.unit), now)
                    } else {
                        0
                    }
                } else if wheel_enabled
                    && ordered_owns_wheel
                    && event.y.is_finite()
                    && event.y != 0.0
                {
                    if event.y < 0.0 { 1 } else { -1 }
                } else {
                    0
                };
                if step != 0 && steps == 0 {
                    steps = step;
                    output.push(DesktopEvent::Scroll { next: step > 0 });
                }
                None
            }
            _ => None,
        };
        if let Some((control, pressed)) = button {
            output.push(DesktopEvent::Button {
                control,
                pressed,
                modifiers: platform_modifiers.unwrap_or_else(|| modifiers(&keys)),
            });
        }
    }
    Some((output, lost))
}

/// Updates physical keyboard state while reconstructing the batch's modifier timeline.
fn change_key(keys: &mut ButtonInput<KeyCode>, key: KeyCode, pressed: bool) {
    if pressed {
        keys.press(key);
    } else {
        keys.release(key);
    }
}

/// Resolves both physical sides of each modifier from the event-time held state.
fn modifiers(keys: &ButtonInput<KeyCode>) -> ModifierChord {
    ModifierChord {
        shift: keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
        control: keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight),
        alt: keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight),
        super_key: keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight),
    }
}
