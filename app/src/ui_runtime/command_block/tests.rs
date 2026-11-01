use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::{AccumulatedMouseMotion, MouseWheel},
    },
    prelude::{App, ButtonInput, KeyCode, MouseButton, Update, Window},
    window::{CursorOptions, PrimaryWindow},
};
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
use client_ui::ui_runtime::{
    UiRuntime, command_block::CommandBlockEdit, presentation::UiPresentationRuntime,
};
use protocol::{FormKind, FormRequestEvent, ServerFormModel, UnsupportedForm};
use ui_engine::test_support::fixture_font;
use ui_screens::command_block_screen::CommandBlockMode;

use super::drive_command_block;

/// A runtime showing the editor over an unedited block.
fn runtime_with_editor() -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    runtime.command_block_mut().open(CommandBlockEdit::load(
        [2, -60, 0],
        CommandBlockMode::Impulse,
        Some(false),
        &world::NbtCompound::default(),
        |_| None,
    ));
    runtime
}

/// Runs one editor frame with Escape going down, and returns whether the key survived it.
fn escape_survives_a_frame(runtime: UiRuntime) -> bool {
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::Escape);
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .add_message::<MouseWheel>()
        .insert_resource(keys)
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<crate::runtime::world::ClientWorld>()
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(UiPresentationResource(
            UiPresentationRuntime::new(fixture_font()).unwrap(),
        ))
        .add_systems(Update, drive_command_block);
    let window = app
        .world_mut()
        .spawn((Window::default(), CursorOptions::default(), PrimaryWindow))
        .id();
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Escape,
        logical_key: Key::Escape,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    app.update();
    let runtime = app.world().resource::<UiRuntimeResource>();
    assert!(runtime.command_block().is_open());
    assert!(!runtime.command_block().exit_requested());
    app.world()
        .resource::<ButtonInput<KeyCode>>()
        .pressed(KeyCode::Escape)
}

#[test]
fn the_open_editor_takes_the_keys() {
    assert!(!escape_survives_a_frame(runtime_with_editor()));
}

#[test]
fn a_server_form_over_the_editor_keeps_the_keys_from_it() {
    let mut runtime = runtime_with_editor();
    let session = runtime.session_id();
    runtime.server_forms_mut().admit(
        FormRequestEvent {
            form_id: 3,
            kind: FormKind::Menu,
            title: None,
            json: "{}".into(),
            model: ServerFormModel::Unsupported(UnsupportedForm::Family),
        },
        1,
        session,
        false,
    );
    assert!(runtime.server_forms().owns_input());
    assert!(escape_survives_a_frame(runtime));
}
