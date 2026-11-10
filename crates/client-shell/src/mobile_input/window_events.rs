//! Ordered native keyboard and mouse input for the primary window.

use bevy::{
    ecs::system::SystemParam,
    input::{
        ButtonState,
        keyboard::KeyboardInput,
        mouse::{MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel},
        touch::TouchPhase,
    },
    prelude::*,
    window::{CursorLeft, CursorMoved, PrimaryWindow, WindowEvent},
};

use super::hid_keys;

/// One native callback, retained in arrival order across input kinds.
pub(super) enum NativeWindowEvent {
    Key {
        usage: u16,
        pressed: bool,
    },
    Motion(Vec2),
    Button {
        button: MouseButton,
        pressed: bool,
    },
    Scroll(Vec2),
    /// Absolute position in physical pixels, or none when hover ends.
    Cursor(Option<Vec2>),
}

/// Native callbacks waiting for the next input update.
#[derive(Resource, Default)]
pub(super) struct PendingWindowEvents(pub Vec<NativeWindowEvent>);

/// Publishes each event to its typed stream and the ordered window stream.
#[derive(SystemParam)]
pub(super) struct WindowInputWriters<'w> {
    keys: MessageWriter<'w, KeyboardInput>,
    buttons: MessageWriter<'w, MouseButtonInput>,
    motion: MessageWriter<'w, MouseMotion>,
    wheel: MessageWriter<'w, MouseWheel>,
    cursor: MessageWriter<'w, CursorMoved>,
    left: MessageWriter<'w, CursorLeft>,
    ordered: MessageWriter<'w, WindowEvent>,
}

/// Forwards callbacks before Bevy processes input, preserving order and IME ownership.
pub(super) fn forward(
    mut pending: ResMut<PendingWindowEvents>,
    window: Option<Single<(Entity, &mut Window), With<PrimaryWindow>>>,
    focus: Option<Res<client_state::CursorFocus>>,
    mut primary_uses_touch: Local<bool>,
    mut messages: WindowInputWriters,
) {
    let Some(window) = window else {
        pending.0.clear();
        return;
    };
    let (entity, mut window) = window.into_inner();
    let locked = focus.is_some_and(|focus| focus.0.grab_free_capture());
    for event in pending.0.drain(..) {
        let event: WindowEvent = match event {
            NativeWindowEvent::Key { usage, pressed } => {
                let Some(key_code) = hid_keys::key_code(usage) else {
                    continue;
                };
                if window.ime_enabled && key_code == KeyCode::Backspace {
                    continue;
                }
                let event = KeyboardInput {
                    key_code,
                    logical_key: hid_keys::logical_key(key_code),
                    state: button_state(pressed),
                    text: None,
                    repeat: false,
                    window: entity,
                };
                messages.keys.write(event.clone());
                event.into()
            }
            NativeWindowEvent::Motion(delta) => {
                let event = MouseMotion { delta };
                messages.motion.write(event);
                event.into()
            }
            NativeWindowEvent::Button { button, pressed } => {
                if button == MouseButton::Left {
                    // UIKit owns unlocked primary clicks through their release.
                    if pressed {
                        *primary_uses_touch = !locked;
                    }
                    if *primary_uses_touch {
                        if !pressed {
                            *primary_uses_touch = false;
                        }
                        continue;
                    }
                }
                let event = MouseButtonInput {
                    button,
                    state: button_state(pressed),
                    window: entity,
                };
                messages.buttons.write(event);
                event.into()
            }
            NativeWindowEvent::Scroll(delta) => {
                let event = MouseWheel {
                    unit: MouseScrollUnit::Line,
                    x: delta.x,
                    y: delta.y,
                    window: entity,
                    phase: TouchPhase::Moved,
                };
                messages.wheel.write(event);
                event.into()
            }
            NativeWindowEvent::Cursor(position) => {
                if locked {
                    continue;
                }
                let previous = window.cursor_position();
                window
                    .bypass_change_detection()
                    .set_physical_cursor_position(position.map(|position| position.as_dvec2()));
                if let Some(position) = window.cursor_position() {
                    let event = CursorMoved {
                        window: entity,
                        position,
                        delta: previous.map(|previous| position - previous),
                    };
                    messages.cursor.write(event.clone());
                    event.into()
                } else {
                    let event = CursorLeft { window: entity };
                    messages.left.write(event.clone());
                    event.into()
                }
            }
        };
        messages.ordered.write(event);
    }
}

/// Converts a native pressed flag to Bevy's button state.
fn button_state(pressed: bool) -> ButtonState {
    if pressed {
        ButtonState::Pressed
    } else {
        ButtonState::Released
    }
}

#[cfg(test)]
mod tests;
