use super::*;
use client_extensions::server_experiences::input::{consume, ordinary_input};
use client_ui::ui_runtime::forms::FormValue;

// A Ctrl release on a frame the consent prompt owned never reaches the form, so a form that kept
// Ctrl held across that frame turned the next plain V into a paste.
#[test]
fn ctrl_released_while_consent_owns_input_leaves_v_typed_text() {
    let Some(CustomFormApp {
        mut app,
        entity,
        edit_box,
        ..
    }) = custom_form_app(|app| {
        app.add_plugins(bevy::diagnostic::FrameCountPlugin)
            .insert_resource(client_state::ConsentInput(false))
            .add_systems(
                Update,
                (consume, drive_server_form_input.run_if(ordinary_input)).chain(),
            );
    })
    else {
        eprintln!(
            "skipping ctrl_released_while_consent_owns_input_leaves_v_typed_text: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    click_at(&mut app, entity, edit_box);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    app.world_mut().write_message(typed(
        entity,
        KeyCode::ControlLeft,
        ButtonState::Pressed,
        None,
    ));
    app.update();

    app.insert_resource(client_state::ConsentInput(true));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ControlLeft);
    app.world_mut().write_message(typed(
        entity,
        KeyCode::ControlLeft,
        ButtonState::Released,
        None,
    ));
    app.update();

    app.insert_resource(client_state::ConsentInput(false));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyV);
    app.world_mut().write_message(typed(
        entity,
        KeyCode::KeyV,
        ButtonState::Pressed,
        Some("v"),
    ));
    app.update();
    assert_eq!(form_values(&app)[1], FormValue::Text("v".into()));
}

// macOS can swallow a Command key-up without a focus change; Bevy then recovers the release with
// the next plain V and appends it after that V, which must still type.
#[test]
fn command_release_recovered_after_v_types_v() {
    let Some(CustomFormApp {
        mut app,
        entity,
        edit_box,
        ..
    }) = custom_form_app(|app| {
        app.add_systems(Update, drive_server_form_input);
    })
    else {
        eprintln!(
            "skipping command_release_recovered_after_v_types_v: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    click_at(&mut app, entity, edit_box);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::SuperLeft);
    app.world_mut().write_message(typed(
        entity,
        KeyCode::SuperLeft,
        ButtonState::Pressed,
        None,
    ));
    app.update();

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyV);
    app.world_mut().write_message(typed(
        entity,
        KeyCode::KeyV,
        ButtonState::Pressed,
        Some("v"),
    ));
    app.world_mut().write_message(typed(
        entity,
        KeyCode::SuperLeft,
        ButtonState::Released,
        None,
    ));
    app.update();
    assert_eq!(form_values(&app)[1], FormValue::Text("v".into()));
}
