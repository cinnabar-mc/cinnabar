use super::*;
use bevy::{input::touch::Touches, prelude::*};

#[test]
fn mod_reservations_remove_ordered_mouse_edges_and_preserve_other_controls() {
    let mut app = App::new();
    app.add_message::<WindowEvent>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .insert_resource(crate::semantic_controls::ReservedControls(vec![
            "MouseRight".into(),
            "KeyW".into(),
        ]))
        .add_systems(Update, collect_raw_input);
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            CursorOptions {
                grab_mode: bevy::window::CursorGrabMode::Locked,
                visible: false,
                ..Default::default()
            },
            PrimaryWindow,
        ))
        .id();
    for button in [MouseButton::Right, MouseButton::Left] {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            app.world_mut().write_message(WindowEvent::MouseButtonInput(
                bevy::input::mouse::MouseButtonInput {
                    button,
                    state,
                    window,
                },
            ));
        }
    }
    for key_code in [KeyCode::KeyW, KeyCode::KeyA] {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            app.world_mut().write_message(WindowEvent::KeyboardInput(
                bevy::input::keyboard::KeyboardInput {
                    key_code,
                    logical_key: bevy::input::keyboard::Key::Character("x".into()),
                    state,
                    text: None,
                    repeat: false,
                    window,
                },
            ));
        }
    }
    app.update();
    let pending = app.world().resource::<PendingDeviceFrame>();
    let events = pending
        .frame
        .as_ref()
        .unwrap()
        .desktop_events
        .as_ref()
        .unwrap();
    let expected: Vec<_> = [
        PhysicalControl::MouseButton(mouse_button_code(MouseButton::Left).unwrap()),
        PhysicalControl::KeyboardUsage(keyboard_usage(KeyCode::KeyA).unwrap()),
    ]
    .into_iter()
    .flat_map(|control| {
        [true, false].map(|pressed| DesktopEvent::Button {
            control,
            pressed,
            modifiers: ModifierChord::default(),
        })
    })
    .collect();
    assert_eq!(events.events(), expected);
}

/// A platform click drained after a same-frame modifier press and release keeps the modifiers the
/// platform reported when it happened.
#[test]
fn platform_buttons_use_their_event_time_modifiers() {
    let mut app = App::new();
    app.add_message::<WindowEvent>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .add_systems(Update, collect_raw_input);
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            CursorOptions {
                grab_mode: bevy::window::CursorGrabMode::Locked,
                visible: false,
                ..Default::default()
            },
            PrimaryWindow,
        ))
        .id();
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(WindowEvent::KeyboardInput(
            bevy::input::keyboard::KeyboardInput {
                key_code: KeyCode::ShiftLeft,
                logical_key: bevy::input::keyboard::Key::Shift,
                state,
                text: None,
                repeat: false,
                window,
            },
        ));
    }
    let click = bevy::input::mouse::MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    };
    app.world_mut()
        .write_message(WindowEvent::MouseButtonInput(click));
    let shift = ModifierChord {
        shift: true,
        ..ModifierChord::default()
    };
    app.insert_resource(crate::platform_devices::PlatformButtonModifiers(vec![(
        click, shift,
    )]));
    app.update();

    let pending = app.world().resource::<PendingDeviceFrame>();
    let events = pending
        .frame
        .as_ref()
        .unwrap()
        .desktop_events
        .as_ref()
        .unwrap();
    assert_eq!(
        events.events().last(),
        Some(&DesktopEvent::Button {
            control: PhysicalControl::MouseButton(mouse_button_code(MouseButton::Left).unwrap()),
            pressed: true,
            modifiers: shift,
        })
    );
}

/// A reserved platform button still consumes its modifier record, so a later unreserved click in
/// the same frame keeps its own event-time modifiers.
#[test]
fn reserved_platform_buttons_keep_modifier_records_aligned() {
    let mut app = App::new();
    app.add_message::<WindowEvent>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .insert_resource(crate::semantic_controls::ReservedControls(vec![
            "MouseRight".into(),
        ]))
        .add_systems(Update, collect_raw_input);
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            CursorOptions {
                grab_mode: bevy::window::CursorGrabMode::Locked,
                visible: false,
                ..Default::default()
            },
            PrimaryWindow,
        ))
        .id();
    let press = |button| bevy::input::mouse::MouseButtonInput {
        button,
        state: ButtonState::Pressed,
        window,
    };
    let (reserved, click) = (press(MouseButton::Right), press(MouseButton::Left));
    for event in [reserved, click] {
        app.world_mut()
            .write_message(WindowEvent::MouseButtonInput(event));
    }
    let shift = ModifierChord {
        shift: true,
        ..ModifierChord::default()
    };
    app.insert_resource(crate::platform_devices::PlatformButtonModifiers(vec![
        (reserved, ModifierChord::default()),
        (click, shift),
    ]));
    app.update();

    let pending = app.world().resource::<PendingDeviceFrame>();
    let events = pending
        .frame
        .as_ref()
        .unwrap()
        .desktop_events
        .as_ref()
        .unwrap();
    assert_eq!(
        events.events(),
        [DesktopEvent::Button {
            control: PhysicalControl::MouseButton(mouse_button_code(MouseButton::Left).unwrap()),
            pressed: true,
            modifiers: shift,
        }]
    );
}
