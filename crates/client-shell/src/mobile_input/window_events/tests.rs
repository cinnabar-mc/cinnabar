use bevy::{
    ecs::message::Message,
    input::{InputPlugin, touch::TouchInput},
};

use super::*;

/// Builds the native input boundary without a platform window or renderer.
fn app() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(InputPlugin)
        .add_message::<WindowEvent>()
        .add_message::<CursorMoved>()
        .add_message::<CursorLeft>()
        .init_resource::<PendingWindowEvents>()
        .init_resource::<client_state::CursorFocus>()
        .add_systems(PreUpdate, forward.before(bevy::input::InputSystems));
    let mut window = Window::default();
    window.resolution.set_scale_factor_override(Some(2.0));
    let entity = app.world_mut().spawn((window, PrimaryWindow)).id();
    (app, entity)
}

/// Forwards a callback batch through the same system used on iOS.
fn send(app: &mut App, events: impl IntoIterator<Item = NativeWindowEvent>) {
    app.world_mut()
        .resource_mut::<PendingWindowEvents>()
        .0
        .extend(events);
    app.update();
}

/// Takes the messages published to one stream without replaying previous frames.
fn take<M: Message>(app: &mut App) -> Vec<M> {
    app.world_mut()
        .resource_mut::<Messages<M>>()
        .drain()
        .collect()
}

/// Makes a physical key callback using the shared HID mapping.
fn key(key: KeyCode, pressed: bool) -> NativeWindowEvent {
    NativeWindowEvent::Key {
        usage: crate::physical_input::keyboard_usage(key).unwrap(),
        pressed,
    }
}

#[test]
fn keyboard_mouse_and_scroll_reach_both_streams_in_callback_order() {
    let (mut app, window) = app();
    app.world_mut()
        .resource_mut::<client_state::CursorFocus>()
        .0
        .set_grab_free_capture(true);
    send(
        &mut app,
        [
            NativeWindowEvent::Button {
                button: MouseButton::Left,
                pressed: true,
            },
            key(KeyCode::ShiftLeft, true),
            NativeWindowEvent::Motion(Vec2::new(3.0, -2.0)),
            NativeWindowEvent::Scroll(Vec2::new(0.0, -1.0)),
            key(KeyCode::ShiftLeft, false),
            NativeWindowEvent::Button {
                button: MouseButton::Left,
                pressed: false,
            },
        ],
    );
    let keys = take::<KeyboardInput>(&mut app);
    let buttons = take::<MouseButtonInput>(&mut app);
    let motion = take::<MouseMotion>(&mut app);
    let wheel = take::<MouseWheel>(&mut app);
    assert_eq!(keys.len(), 2);
    assert_eq!(buttons.len(), 2);
    assert_eq!(motion.len(), 1);
    assert_eq!(wheel.len(), 1);
    assert_eq!(keys[0].key_code, KeyCode::ShiftLeft);
    assert_eq!(keys[0].state, ButtonState::Pressed);
    assert_eq!(keys[1].state, ButtonState::Released);
    assert!(keys.iter().all(|key| key.window == window && !key.repeat));
    assert_eq!(buttons[0].window, window);
    assert_eq!(buttons[0].state, ButtonState::Pressed);
    assert_eq!(buttons[1].state, ButtonState::Released);
    assert_eq!(motion[0].delta, Vec2::new(3.0, -2.0));
    assert_eq!(wheel[0].y, -1.0);
    assert_eq!(
        take::<WindowEvent>(&mut app),
        [
            buttons[0].into(),
            keys[0].clone().into(),
            motion[0].into(),
            wheel[0].into(),
            keys[1].clone().into(),
            buttons[1].into(),
        ]
    );
}

#[test]
fn unlocked_primary_click_preserves_binding_state_and_ordered_form_input() {
    let (mut app, window) = app();
    let position = Vec2::new(120.0, 80.0);
    for (pressed, phase) in [(true, TouchPhase::Started), (false, TouchPhase::Ended)] {
        // A real finger remains independent of the native mouse.
        app.world_mut().write_message(TouchInput {
            phase,
            position,
            window,
            force: None,
            id: 7,
        });
        send(
            &mut app,
            [
                NativeWindowEvent::Cursor(Some(position * 2.0)),
                NativeWindowEvent::Button {
                    button: MouseButton::Left,
                    pressed,
                },
            ],
        );
        let touches = app.world().resource::<Touches>();
        let touch = if pressed {
            touches.get_pressed(7)
        } else {
            touches.get_released(7)
        };
        assert_eq!(touch.unwrap().position(), position);
        let button = MouseButtonInput {
            button: MouseButton::Left,
            state: button_state(pressed),
            window,
        };
        assert_eq!(take::<MouseButtonInput>(&mut app), [button]);
        let cursor = take::<CursorMoved>(&mut app);
        assert_eq!(
            take::<WindowEvent>(&mut app),
            [cursor[0].clone().into(), button.into()]
        );
        let mouse = app.world().resource::<ButtonInput<MouseButton>>();
        assert_eq!(mouse.pressed(MouseButton::Left), pressed);
        assert_eq!(mouse.just_pressed(MouseButton::Left), pressed);
        assert_eq!(mouse.just_released(MouseButton::Left), !pressed);
    }
}

#[test]
fn primary_click_keeps_both_edges_when_pointer_lock_changes_before_release() {
    for locked in [false, true] {
        let (mut app, window) = app();
        app.world_mut()
            .resource_mut::<client_state::CursorFocus>()
            .0
            .set_grab_free_capture(locked);
        send(
            &mut app,
            [NativeWindowEvent::Button {
                button: MouseButton::Left,
                pressed: true,
            }],
        );
        assert!(
            app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
        app.world_mut()
            .resource_mut::<client_state::CursorFocus>()
            .0
            .set_grab_free_capture(!locked);
        send(
            &mut app,
            [NativeWindowEvent::Button {
                button: MouseButton::Left,
                pressed: false,
            }],
        );
        let buttons = take::<MouseButtonInput>(&mut app);
        assert_eq!(
            buttons,
            [
                MouseButtonInput {
                    button: MouseButton::Left,
                    state: ButtonState::Pressed,
                    window,
                },
                MouseButtonInput {
                    button: MouseButton::Left,
                    state: ButtonState::Released,
                    window,
                },
            ]
        );
        assert_eq!(
            take::<WindowEvent>(&mut app),
            buttons
                .into_iter()
                .map(WindowEvent::from)
                .collect::<Vec<_>>()
        );
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
    }
}

#[test]
fn unlocked_secondary_clicks_reach_both_mouse_streams() {
    let (mut app, window) = app();
    send(
        &mut app,
        [true, false].map(|pressed| NativeWindowEvent::Button {
            button: MouseButton::Right,
            pressed,
        }),
    );
    let buttons = take::<MouseButtonInput>(&mut app);
    assert_eq!(buttons.len(), 2);
    assert_eq!(buttons[0].window, window);
    assert_eq!(buttons[0].button, MouseButton::Right);
    assert_eq!(buttons[0].state, ButtonState::Pressed);
    assert_eq!(buttons[1].state, ButtonState::Released);
    assert_eq!(
        take::<WindowEvent>(&mut app),
        buttons
            .into_iter()
            .map(WindowEvent::from)
            .collect::<Vec<_>>()
    );
}

#[test]
fn unlocked_hover_supplies_the_scaled_menu_pointer_before_scrolling() {
    let (mut app, window) = app();
    send(
        &mut app,
        [
            NativeWindowEvent::Cursor(Some(Vec2::new(240.0, 160.0))),
            NativeWindowEvent::Scroll(Vec2::new(0.0, -1.0)),
        ],
    );
    let position = Vec2::new(120.0, 80.0);
    assert_eq!(
        app.world().get::<Window>(window).unwrap().cursor_position(),
        Some(position)
    );
    let cursor = take::<CursorMoved>(&mut app);
    assert_eq!(
        cursor,
        [CursorMoved {
            window,
            position,
            delta: None
        }]
    );
    let wheel = take::<MouseWheel>(&mut app);
    assert_eq!(wheel.len(), 1);
    assert_eq!(wheel[0].window, window);
    assert_eq!(
        take::<WindowEvent>(&mut app),
        [cursor[0].clone().into(), wheel[0].into()]
    );

    send(
        &mut app,
        [NativeWindowEvent::Cursor(Some(Vec2::new(260.0, 180.0)))],
    );
    assert_eq!(
        take::<CursorMoved>(&mut app)[0].delta,
        Some(Vec2::splat(10.0))
    );
    send(&mut app, [NativeWindowEvent::Cursor(None)]);
    assert_eq!(
        app.world().get::<Window>(window).unwrap().cursor_position(),
        None
    );
    assert_eq!(take::<CursorLeft>(&mut app), [CursorLeft { window }]);
}

#[test]
fn observed_hover_does_not_request_native_window_changes() {
    #[derive(Resource, Default)]
    struct WindowChanges(usize);

    let (mut app, window) = app();
    app.init_resource::<WindowChanges>().add_systems(
        PostUpdate,
        |windows: Query<(), Changed<Window>>, mut changes: ResMut<WindowChanges>| {
            changes.0 = windows.iter().count();
        },
    );
    app.update();
    assert_eq!(app.world().resource::<WindowChanges>().0, 1);
    for position in [
        Some(Vec2::new(240.0, 160.0)),
        Some(Vec2::splat(200.0)),
        None,
    ] {
        send(&mut app, [NativeWindowEvent::Cursor(position)]);
        assert_eq!(
            app.world().get::<Window>(window).unwrap().cursor_position(),
            position.map(|position| position / 2.0)
        );
        assert_eq!(app.world().resource::<WindowChanges>().0, 0);
    }
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .ime_enabled = true;
    send(
        &mut app,
        [NativeWindowEvent::Cursor(Some(Vec2::splat(200.0)))],
    );
    assert_eq!(app.world().resource::<WindowChanges>().0, 1);
}

#[test]
fn pointer_lock_ignores_absolute_hover_and_keeps_relative_motion() {
    let (mut app, window) = app();
    app.world_mut()
        .resource_mut::<client_state::CursorFocus>()
        .0
        .set_grab_free_capture(true);
    send(
        &mut app,
        [
            NativeWindowEvent::Cursor(Some(Vec2::new(240.0, 160.0))),
            NativeWindowEvent::Motion(Vec2::new(2.0, 1.0)),
        ],
    );
    assert_eq!(
        app.world().get::<Window>(window).unwrap().cursor_position(),
        None
    );
    assert!(take::<CursorMoved>(&mut app).is_empty());
    assert_eq!(take::<MouseMotion>(&mut app)[0].delta, Vec2::new(2.0, 1.0));
}

#[test]
fn ime_backspace_and_unknown_usages_are_omitted_from_both_streams() {
    let (mut app, window) = app();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .ime_enabled = true;
    send(
        &mut app,
        [
            key(KeyCode::Backspace, true),
            NativeWindowEvent::Key {
                usage: 0,
                pressed: true,
            },
            key(KeyCode::Enter, true),
        ],
    );
    let keys = take::<KeyboardInput>(&mut app);
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].key_code, KeyCode::Enter);
    assert_eq!(take::<WindowEvent>(&mut app), [keys[0].clone().into()]);
}
