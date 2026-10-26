//! The platform Back key or gesture cancels screens like Menu Cancel, whatever that is bound to.

use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::{ButtonInput, MessageReader, MouseButton, ResMut},
};
use client_shell::physical_input::PLATFORM_BACK;

/// Whether a key event is the platform Back key, which Android also sends for its Back gesture.
pub(crate) fn is_platform_back(input: &KeyboardInput) -> bool {
    input.logical_key == Key::BrowserBack
}

/// Holds the platform Back press while the key is down, so forms, menus and other screens share
/// the Menu Cancel route. Runs after input collection; key repeats press nothing. Chat and
/// inventories route the key itself at its place among the frame's keys.
pub(crate) fn press_mouse_back_for_platform_back(
    mut keyboard: MessageReader<KeyboardInput>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
) {
    for input in keyboard.read() {
        if !is_platform_back(input) {
            continue;
        }
        match input.state {
            ButtonState::Pressed if !input.repeat => buttons.press(PLATFORM_BACK),
            ButtonState::Pressed => {}
            ButtonState::Released => buttons.release(PLATFORM_BACK),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        input::{InputPlugin, InputSystems, keyboard::NativeKeyCode},
        prelude::*,
    };

    fn back(window: Entity, state: ButtonState, repeat: bool) -> KeyboardInput {
        KeyboardInput {
            key_code: KeyCode::Unidentified(NativeKeyCode::Android(4)),
            logical_key: Key::BrowserBack,
            state,
            text: None,
            repeat,
            window,
        }
    }

    /// Android's Back is pressed once as the platform Back press, ignores repeats, and lifts with
    /// the key.
    #[test]
    fn platform_back_presses_mouse_back_once() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin)).add_systems(
            PreUpdate,
            press_mouse_back_for_platform_back.after(InputSystems),
        );
        let window = app.world_mut().spawn_empty().id();
        app.world_mut()
            .write_message(back(window, ButtonState::Pressed, false));
        app.update();
        let buttons = app.world().resource::<ButtonInput<MouseButton>>();
        assert!(buttons.just_pressed(PLATFORM_BACK));
        app.world_mut()
            .write_message(back(window, ButtonState::Pressed, true));
        app.update();
        let buttons = app.world().resource::<ButtonInput<MouseButton>>();
        assert!(buttons.pressed(PLATFORM_BACK));
        assert!(!buttons.just_pressed(PLATFORM_BACK));
        app.world_mut()
            .write_message(back(window, ButtonState::Released, false));
        app.update();
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(PLATFORM_BACK)
        );
    }
}
