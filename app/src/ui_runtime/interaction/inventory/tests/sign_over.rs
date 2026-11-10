//! A sign editor opened over an inventory keeps the frame's input from it.
use super::ordered::personal_app;
use super::*;
use crate::ui_runtime::{drive_chat_keyboard_input, drive_sign_editor};
use bevy::input::keyboard::KeyboardInput;
use client_ui::ui_runtime::sign_editor::SignEdit;

/// The inventory app with a sign editor open over the inventory, whose answer can be sent.
fn sign_over_inventory() -> (App, Entity) {
    let (mut app, window) = personal_app(2);
    let (network, packets) = client_runtime::network::NetworkHandle::<
        crate::runtime::network::resource_packs::PackApplication,
    >::stub_capturing_packets();
    // Its packets are unread, and the capture stays alive so the sign's answer can be sent.
    Box::leak(Box::new(packets));
    app.insert_resource(network)
        .init_resource::<crate::runtime::world::ClientWorld>()
        .init_resource::<client_shell::ui_resources::FrameInputOwner>()
        .add_systems(
            Update,
            (
                crate::ui_runtime::input_owner::decide_input_owner,
                drive_sign_editor,
            )
                .chain()
                .before(drive_chat_keyboard_input),
        );
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .sign_editor_mut()
        .open(SignEdit::new([0, 64, 0], true, Default::default()));
    (app, window)
}

fn pending_requests(app: &App) -> usize {
    let player = app.world().resource::<client_state::PlayerRuntime>();
    player.inventory.ledger().pending_request_count()
}

#[test]
fn escape_closing_a_sign_leaves_the_inventory_under_it_open() {
    let (mut app, window) = sign_over_inventory();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
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
    assert!(!runtime.sign_editor().is_open());
    assert!(runtime.inventory_open());
}

#[test]
fn a_click_on_a_sign_leaves_the_inventory_under_it_alone() {
    let (mut app, _) = sign_over_inventory();
    let before = pending_requests(&app);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert_eq!(pending_requests(&app), before);
}
