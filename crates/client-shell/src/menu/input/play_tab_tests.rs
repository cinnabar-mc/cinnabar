use super::mode::MenuInputMode;
use super::*;
use bevy::{input::keyboard::Key, prelude::*};
use launcher::menu::{MenuAction, MenuScreen};

#[test]
fn bracket_shortcuts_use_keyboard_hints_after_gamepad_or_touch_input() {
    for mode in [MenuInputMode::Gamepad, MenuInputMode::Touch] {
        for (key_code, text, target) in [
            (KeyCode::BracketLeft, "[", MenuScreen::Play),
            (KeyCode::BracketRight, "]", MenuScreen::Servers),
        ] {
            let mut menu = MenuRuntime::new(true, 2, "Player".into());
            menu.activate(MenuAction::Navigate(MenuScreen::Social));
            menu.input_mode = mode;
            let mut app = App::new();
            app.add_message::<KeyboardInput>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<ButtonInput<MouseButton>>()
                .init_resource::<Touches>()
                .init_resource::<MenuClipboard>()
                .insert_resource(menu)
                .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
                    1,
                )))
                .insert_resource(UiPresentationResource(
                    client_ui::test_support::mini_engine_presentation(),
                ))
                .add_systems(Update, drive_menu_input);
            let window = app
                .world_mut()
                .spawn((
                    Window {
                        focused: true,
                        ..Default::default()
                    },
                    CursorOptions::default(),
                    PrimaryWindow,
                ))
                .id();
            app.world_mut().write_message(KeyboardInput {
                key_code,
                logical_key: Key::Character(text.into()),
                state: ButtonState::Pressed,
                text: Some(text.into()),
                repeat: false,
                window,
            });
            app.update();
            let view = app.world().resource::<MenuRuntime>().view();
            assert_eq!(view.screen, target);
            assert!(view.navigation_focus_visible);
            assert!(!view.gamepad_input);
            assert!(!view.touch_input);
        }
    }
}
