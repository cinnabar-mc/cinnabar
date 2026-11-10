//! Gameplay wheel observations follow final screen and cursor ownership.

use super::*;
use crate::modding::{
    begin_mod_frame, drive_mod, input::prepare_mod_input, interaction::ModInteraction,
    registration::HostMetadata,
};
use bevy::window::CursorGrabMode;
use std::path::Path;

/// Loads two wheel observers and runs the production input and callback owners in order.
fn scroll_app(directory: &Path) -> (App, Entity) {
    let (mut app, window) = input_app();
    app.world_mut()
        .resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
            ui.close_inventory(&mut world.resource_mut::<client_state::PlayerRuntime>());
        });
    let package = include_str!("../../../../../crates/mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let source =
        include_str!("../../../../../crates/client-extensions/src/modding/scroll_probe.wat")
            .replace("$INPUT", &format!("{name}/input@{version}"))
            .replace("$HUD", &format!("{name}/hud@{version}"))
            .replace("$CLAIM_COUNT", "0")
            .replace("$FRAME_END", "");
    let mut hosts = (0..2).map(|index| {
        let path = directory.join(format!("scroll-{index}.wat"));
        std::fs::write(&path, &source).unwrap();
        let mut host = ModHost::load_with_grants(
            &path,
            mod_host::ModGrants {
                controls: true,
                hud: true,
                ..Default::default()
            },
        )
        .unwrap();
        host.bind_drawing_session(mod_api::drawing::DrawingSession::default());
        assert!(host.reload_if_changed().unwrap());
        host
    });
    let mut runtime = ModRuntime::new(hosts.next().unwrap(), HostMetadata::environment(0));
    runtime.push_host(hosts.next().unwrap(), HostMetadata::environment(1));
    app.insert_resource(runtime)
        .insert_resource(crate::environment::VisualTimeOverride(None))
        .insert_resource(client_state::DrivenInput)
        .init_resource::<ModInteraction>()
        .init_resource::<crate::modding::pointer::Pending>()
        .add_systems(
            Update,
            (
                begin_mod_frame,
                prepare_mod_input,
                drive_chat_keyboard_input,
                drive_mod,
            )
                .chain(),
        );
    (app, window)
}

/// Queues a line-wheel event for the same frame as a screen or capture transition.
fn queue_scroll(app: &mut App, window: Entity) {
    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: 1.0,
        window,
        phase: bevy::input::touch::TouchPhase::Moved,
    });
}

/// Checks what every guest read through its actual wheel import and frame callback.
fn assert_scroll_label(app: &App, expected: &str) {
    let runtime = app.world().resource::<ModRuntime>();
    for index in 0..runtime.hosts.host_count() {
        let host = runtime.hosts.host(index);
        assert!(host.is_active());
        assert_eq!(host.label(), Some(expected), "wheel observer {index}");
    }
}

#[test]
fn gameplay_scroll_is_cleared_when_a_screen_opens_in_the_same_frame() {
    for key in [KeyCode::KeyE, KeyCode::KeyT] {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, window) = scroll_app(directory.path());
        queue_scroll(&mut app, window);
        app.update();
        assert_scroll_label(&app, "scroll");

        queue_key(&mut app, window, key, ButtonState::Pressed, None);
        queue_scroll(&mut app, window);
        app.update();
        let ui = app.world().resource::<UiRuntimeResource>();
        assert_eq!(ui.inventory_open(), key == KeyCode::KeyE);
        assert_eq!(ui.chat_focused(), key == KeyCode::KeyT);
        assert_scroll_label(&app, "none");

        queue_key(
            &mut app,
            window,
            KeyCode::Escape,
            ButtonState::Pressed,
            None,
        );
        app.update();
        assert_scroll_label(&app, "none");
        queue_scroll(&mut app, window);
        app.update();
        assert_scroll_label(&app, "scroll");
    }
}

#[test]
fn gameplay_scroll_is_cleared_when_cursor_capture_is_released_after_sampling() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, window) = scroll_app(directory.path());
    app.world_mut()
        .remove_resource::<client_state::DrivenInput>();
    let mut focus = client_state::CursorFocus::default();
    focus.set_grab_free_capture(true);
    app.insert_resource(focus);
    {
        let mut cursor = app.world_mut().get_mut::<CursorOptions>(window).unwrap();
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
    queue_scroll(&mut app, window);
    app.update();
    assert_scroll_label(&app, "scroll");
    app.add_systems(
        Update,
        (|cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
          mut focus: ResMut<client_state::CursorFocus>| {
            let mut cursor = cursor.into_inner();
            client_shell::physical_input::release_cursor(&mut cursor);
            focus.set_grab_free_capture(false);
        })
        .after(drive_chat_keyboard_input)
        .before(drive_mod),
    );
    queue_scroll(&mut app, window);
    app.update();
    assert!(!app.world().resource::<UiRuntimeResource>().inventory_open());
    assert_scroll_label(&app, "none");
}
