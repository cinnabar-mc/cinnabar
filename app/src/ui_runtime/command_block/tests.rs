use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput, NativeKey},
        mouse::{AccumulatedMouseMotion, MouseWheel},
    },
    prelude::{App, ButtonInput, Entity, KeyCode, MouseButton, Update, Vec2, Window},
    window::{CursorOptions, PrimaryWindow},
};
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
use client_ui::{
    test_support::mini_engine_presentation,
    ui_runtime::{
        UiRuntime,
        command_block::CommandBlockEdit,
        presentation::{UiPresentationRuntime, forms::ServerUiPack},
    },
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

/// An app running only the editor's input system.
fn editor_app(runtime: UiRuntime, presentation: UiPresentationRuntime) -> (App, Entity) {
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .add_message::<MouseWheel>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<crate::runtime::world::ClientWorld>()
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(UiPresentationResource(presentation))
        .add_systems(Update, drive_command_block);
    let window = app
        .world_mut()
        .spawn((Window::default(), CursorOptions::default(), PrimaryWindow))
        .id();
    (app, window)
}

/// Delivers one key edge the way the input plugin does, into the button state and as a message.
fn key_edge(app: &mut App, window: Entity, key_code: KeyCode, text: Option<&str>, down: bool) {
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    if down {
        keys.press(key_code);
    } else {
        keys.release(key_code);
    }
    app.world_mut().write_message(KeyboardInput {
        key_code,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state: if down {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        },
        text: text.map(Into::into),
        repeat: false,
        window,
    });
}

/// Runs one editor frame with Escape going down, and returns whether the key survived it.
fn escape_survives_a_frame(runtime: UiRuntime) -> bool {
    let presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let (mut app, window) = editor_app(runtime, presentation);
    key_edge(&mut app, window, KeyCode::Escape, None, true);
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

/// A command block screen holding only a command box filling the top left of the screen.
const EDITOR_SCREEN: &[u8] = br#"{
    "namespace": "command_block",
    "command_block_screen": {
        "type": "screen",
        "controls": [{ "command": {
            "type": "edit_box", "size": [200, 30], "anchor_from": "top_left", "anchor_to": "top_left",
            "text_box_name": "command_block.command_text", "text_control": "display",
            "max_length": 32500,
            "button_mappings": [{ "from_button_id": "button.menu_select",
                "to_button_id": "button.text_edit_box_selected",
                "mapping_type": "pressed", "handle_select": true, "handle_deselect": false }],
            "controls": [{ "display": { "type": "label", "text": "", "size": [200, 30] } }]
        } }]
    }
}"#;

/// The editor drawn from [`EDITOR_SCREEN`], with its command box selected by a click.
fn app_typing_into_the_command_box() -> (App, Entity) {
    let player = player_state::PlayerState::new(1);
    let runtime = runtime_with_editor();
    let mut presentation = mini_engine_presentation();
    presentation.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{ "ui_defs": ["ui/command_block_screen.json"] }"#.to_vec(),
            ),
            (
                "ui/command_block_screen.json".into(),
                EDITOR_SCREEN.to_vec(),
            ),
        ]],
        ..Default::default()
    });
    presentation
        .build(
            &player,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let frame = presentation
        .command_block_frame()
        .expect("the editor drew its screen")
        .clone();
    let command = frame
        .hits
        .iter()
        .find(|hit| hit.control_name.as_deref() == Some("command_block.command_text"))
        .expect("the screen has a command box");
    let center = Vec2::new(
        (command.rect.x + command.rect.w / 2.0) as f32 * frame.scale + frame.origin[0],
        (command.rect.y + command.rect.h / 2.0) as f32 * frame.scale + frame.origin[1],
    );
    let (mut app, window) = editor_app(runtime, presentation);
    app.world_mut()
        .query::<&mut Window>()
        .single_mut(app.world_mut())
        .unwrap()
        .set_cursor_position(Some(center));
    let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
    mouse.press(MouseButton::Left);
    app.update();
    let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
    mouse.release(MouseButton::Left);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    assert_eq!(typed_command(&app), "");
    (app, window)
}

/// Types `a` as its own frame and returns the command text after it.
fn type_a(app: &mut App, window: Entity) -> String {
    key_edge(app, window, KeyCode::KeyA, Some("a"), true);
    app.update();
    key_edge(app, window, KeyCode::KeyA, None, false);
    app.update();
    typed_command(app)
}

fn typed_command(app: &App) -> String {
    let runtime = app.world().resource::<UiRuntimeResource>();
    let edit = runtime
        .command_block()
        .active()
        .expect("the editor is open");
    edit.input().command.to_owned()
}

/// The keys reset ending each editor frame doesn't release a modifier the player still holds.
#[test]
fn control_held_since_an_earlier_frame_still_makes_a_shortcut() {
    let (mut app, window) = app_typing_into_the_command_box();
    key_edge(&mut app, window, KeyCode::ControlLeft, None, true);
    app.update();
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::ControlLeft)
    );
    assert_eq!(type_a(&mut app, window), "");
    key_edge(&mut app, window, KeyCode::ControlLeft, None, false);
    app.update();
    assert_eq!(type_a(&mut app, window), "a");
}

/// A modifier already down when the editor first sees the keys has no press message to read.
#[test]
fn control_held_before_any_message_still_makes_a_shortcut() {
    let (mut app, window) = app_typing_into_the_command_box();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    app.update();
    assert_eq!(type_a(&mut app, window), "");
}

/// A release that happens while the window is away never arrives, so nothing stays held.
#[test]
fn losing_focus_forgets_the_held_modifiers() {
    let (mut app, window) = app_typing_into_the_command_box();
    key_edge(&mut app, window, KeyCode::ControlLeft, None, true);
    app.update();
    let set_focus = |app: &mut App, focused| {
        app.world_mut()
            .query::<&mut Window>()
            .single_mut(app.world_mut())
            .unwrap()
            .focused = focused;
    };
    set_focus(&mut app, false);
    app.update();
    set_focus(&mut app, true);
    assert_eq!(type_a(&mut app, window), "a");
}

/// A batch of key edges in one frame takes each press's modifiers from when it arrived.
#[test]
fn a_modifier_applies_only_to_the_presses_made_while_it_was_down() {
    let (mut app, window) = app_typing_into_the_command_box();
    key_edge(&mut app, window, KeyCode::ControlLeft, None, true);
    key_edge(&mut app, window, KeyCode::KeyA, Some("a"), true);
    key_edge(&mut app, window, KeyCode::ControlLeft, None, false);
    app.update();
    assert_eq!(typed_command(&app), "");

    key_edge(&mut app, window, KeyCode::KeyA, None, false);
    key_edge(&mut app, window, KeyCode::KeyA, Some("a"), true);
    key_edge(&mut app, window, KeyCode::ControlLeft, None, true);
    app.update();
    assert_eq!(typed_command(&app), "a");
}
