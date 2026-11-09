use super::{
    PendingDeviceFrame, TRANSLATED_GAMEPAD_BUTTONS, admits_input_mode, collect_raw_input,
    input_source_gates, keyboard_usage, mouse_button_code, route_semantic_input,
    select_lowest_by_key,
};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::input::gamepad::GamepadButton;
use bevy::prelude::{KeyCode, MouseButton};
use bevy::{
    input::{ButtonInput, gamepad::Gamepad, mouse::AccumulatedMouseMotion, touch::Touches},
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use client_shell::physical_input::DESKTOP_KEYS;
use semantic_input::{
    Action, ControlSettings, ControllerFrame, InputMode, MAX_CONTROLLERS, MAX_TOUCH_CONTACTS,
    PhysicalControl, TouchContact,
};

use crate::semantic_controls::{SemanticInputRuntime, SemanticRouteState, SemanticTouchTargets};

const TRANSLATED_MOUSE_BUTTONS: &[MouseButton] = &[
    MouseButton::Left,
    MouseButton::Right,
    MouseButton::Middle,
    MouseButton::Back,
    MouseButton::Forward,
];

/// Family-level guard: every default binding must name a physical control
/// the app can actually emit, so a future binding cannot silently reintroduce
/// an unreachable control.
///
/// Touch hit IDs are deliberately out of scope: a binding can name a valid
/// hit ID that no on-screen region ever assigns, which this cannot see.
/// Touch reachability is tracked as an open gap, not proven here.
#[test]
fn default_binding_reachability_is_explicit_for_every_device_family() {
    let usages = DESKTOP_KEYS
        .iter()
        .filter_map(|(key, _)| keyboard_usage(*key))
        .collect::<Vec<_>>();
    let buttons = TRANSLATED_MOUSE_BUTTONS
        .iter()
        .filter_map(|button| mouse_button_code(*button))
        .collect::<Vec<_>>();
    let gamepad = TRANSLATED_GAMEPAD_BUTTONS
        .iter()
        .map(|(code, _)| *code)
        .collect::<Vec<_>>();

    for binding in ControlSettings::default().bindings() {
        let action = binding.action;
        match binding.chord.control {
            PhysicalControl::KeyboardUsage(code) => assert!(
                usages.contains(&code),
                "{action:?} is bound to keyboard usage {code:#04x}, which keyboard_usage never emits"
            ),
            PhysicalControl::MouseButton(button) => assert!(
                buttons.contains(&button),
                "{action:?} is bound to mouse button {button}, which mouse_button_code never emits"
            ),
            PhysicalControl::GamepadButton(button) => assert!(
                gamepad.contains(&button),
                "{action:?} is bound to gamepad button {button}, which gamepad_button_codes never emits"
            ),
            PhysicalControl::MouseAxis(_) | PhysicalControl::GamepadAxis { .. } => {}
            PhysicalControl::TouchControl(hit_id) => assert!(
                semantic_input::TouchControlLayout::default().contains(hit_id),
                "{action:?} is bound to touch control {hit_id}, which the default layout lacks"
            ),
        }
    }
}

#[test]
fn additional_keys_use_their_usb_keyboard_usages() {
    for (key, expected) in [
        (KeyCode::CapsLock, 0x39),
        (KeyCode::PrintScreen, 0x46),
        (KeyCode::ScrollLock, 0x47),
        (KeyCode::Pause, 0x48),
        (KeyCode::NumLock, 0x53),
        (KeyCode::NumpadDivide, 0x54),
        (KeyCode::NumpadMultiply, 0x55),
        (KeyCode::NumpadSubtract, 0x56),
        (KeyCode::NumpadAdd, 0x57),
        (KeyCode::NumpadEnter, 0x58),
        (KeyCode::Numpad1, 0x59),
        (KeyCode::Numpad2, 0x5a),
        (KeyCode::Numpad3, 0x5b),
        (KeyCode::Numpad4, 0x5c),
        (KeyCode::Numpad5, 0x5d),
        (KeyCode::Numpad6, 0x5e),
        (KeyCode::Numpad7, 0x5f),
        (KeyCode::Numpad8, 0x60),
        (KeyCode::Numpad9, 0x61),
        (KeyCode::Numpad0, 0x62),
        (KeyCode::NumpadDecimal, 0x63),
        (KeyCode::IntlBackslash, 0x64),
        (KeyCode::ContextMenu, 0x65),
        (KeyCode::NumpadEqual, 0x67),
        (KeyCode::F13, 0x68),
        (KeyCode::F14, 0x69),
        (KeyCode::F15, 0x6a),
        (KeyCode::F16, 0x6b),
        (KeyCode::F17, 0x6c),
        (KeyCode::F18, 0x6d),
        (KeyCode::F19, 0x6e),
        (KeyCode::F20, 0x6f),
        (KeyCode::F21, 0x70),
        (KeyCode::F22, 0x71),
        (KeyCode::F23, 0x72),
        (KeyCode::F24, 0x73),
        (KeyCode::NumpadComma, 0x85),
    ] {
        assert_eq!(keyboard_usage(key), Some(expected), "{key:?}");
    }
}

/// Every key this layer translates must produce a distinct HID usage, so a
/// mapping typo cannot quietly alias two keys onto one action.
#[test]
fn translated_keys_map_to_distinct_usages() {
    let mut usages = DESKTOP_KEYS
        .iter()
        .filter_map(|(key, _)| keyboard_usage(*key))
        .collect::<Vec<_>>();
    let translated = usages.len();
    usages.sort_unstable();
    usages.dedup();
    assert_eq!(usages.len(), translated);
}

#[test]
fn mapped_keys_drive_sneak_only_while_physically_held() {
    use crate::semantic_controls::SemanticInputRuntime;
    use launcher::menu::settings_options::{KEY_BINDINGS, SettingsOptions};
    use semantic_input::Action;

    let row = KEY_BINDINGS
        .iter()
        .position(|(action, _)| *action == Action::Sneak)
        .unwrap();
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .add_systems(Update, collect_raw_input);
    app.world_mut().spawn((
        Window::default(),
        CursorOptions {
            grab_mode: CursorGrabMode::Locked,
            visible: false,
            ..Default::default()
        },
        PrimaryWindow,
    ));
    let desktop = DESKTOP_KEYS
        .iter()
        .filter_map(|(key, semantic)| Some((*key, semantic.usage()?)));
    for (key, usage) in [
        (KeyCode::CapsLock, 0x39),
        (KeyCode::NumLock, 0x53),
        (KeyCode::ScrollLock, 0x47),
    ]
    .into_iter()
    .chain(desktop)
    {
        let mut settings = SettingsOptions::default();
        assert!(settings.remap(row, PhysicalControl::KeyboardUsage(usage)));
        let mut runtime = SemanticInputRuntime::default();
        runtime
            .replace_bindings(settings.controls().unwrap())
            .unwrap();
        for _ in 0..2 {
            for (pressed, released, held) in [
                (true, false, true),
                (false, false, true),
                (false, true, false),
                (false, false, false),
            ] {
                {
                    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                    keys.clear();
                    if pressed {
                        keys.press(key);
                    }
                    if released {
                        keys.release(key);
                    }
                }
                app.update();
                let frame = app
                    .world_mut()
                    .resource_mut::<PendingDeviceFrame>()
                    .frame
                    .take()
                    .unwrap();
                let keyboard = frame.keyboard_mouse.as_ref().unwrap();
                assert_eq!(keyboard.keys.contains(&usage), held, "{key:?}");
                assert_eq!(
                    keyboard.key_edges.pressed.contains(&usage),
                    pressed,
                    "{key:?}"
                );
                assert_eq!(
                    keyboard.key_edges.released.contains(&usage),
                    released,
                    "{key:?}"
                );
                runtime.route_device_frame(frame).unwrap();
                let snapshot = runtime.finalize_routed_input().unwrap();
                let phase = snapshot.phases[Action::Sneak as usize];
                assert_eq!(
                    (phase.pressed, phase.released, phase.held),
                    (pressed, released, held),
                    "{key:?}"
                );
            }
        }
    }
}

#[test]
fn review_same_frame_released_modifier_keeps_the_preserved_chord() {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .add_systems(Update, collect_raw_input);
    app.world_mut().spawn((
        Window::default(),
        CursorOptions {
            grab_mode: CursorGrabMode::Locked,
            visible: false,
            ..Default::default()
        },
        PrimaryWindow,
    ));
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ShiftLeft);
        keys.press(KeyCode::KeyW);
        keys.release(KeyCode::ShiftLeft);
        keys.release(KeyCode::KeyW);
    }
    app.update();
    let pending = app.world().resource::<PendingDeviceFrame>();
    let keyboard = pending
        .frame
        .as_ref()
        .unwrap()
        .keyboard_mouse
        .as_ref()
        .unwrap();
    let forward = keyboard_usage(KeyCode::KeyW).unwrap();
    assert!(!keyboard.keys.contains(&forward));
    assert!(keyboard.key_edges.pressed.contains(&forward));
    assert!(keyboard.key_edges.released.contains(&forward));
    assert!(keyboard.modifiers.shift);
}

#[test]
fn raw_motion_requires_desktop_capture_but_driven_input_has_no_os_grab() {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<client_state::CursorFocus>()
        .add_systems(Update, collect_raw_input);
    let window = app
        .world_mut()
        .spawn((
            Window::default(),
            CursorOptions {
                grab_mode: CursorGrabMode::Locked,
                visible: false,
                ..Default::default()
            },
            PrimaryWindow,
        ))
        .id();
    app.world_mut()
        .resource_mut::<AccumulatedMouseMotion>()
        .delta = bevy::prelude::Vec2::new(12.0, -4.0);
    app.update();
    assert_eq!(
        app.world()
            .resource::<PendingDeviceFrame>()
            .frame
            .as_ref()
            .unwrap()
            .keyboard_mouse
            .as_ref()
            .unwrap()
            .mouse_motion,
        [12.0, -4.0]
    );
    app.world_mut()
        .resource_mut::<client_state::CursorFocus>()
        .occlusion_changed(true);
    app.update();
    let pending = app.world().resource::<PendingDeviceFrame>();
    assert!(pending.frame.as_ref().unwrap().window_focus_lost);
    assert!(pending.frame.as_ref().unwrap().keyboard_mouse.is_none());
    app.world_mut()
        .get_mut::<CursorOptions>(window)
        .unwrap()
        .grab_mode = CursorGrabMode::None;
    app.world_mut()
        .get_mut::<CursorOptions>(window)
        .unwrap()
        .visible = true;
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.init_resource::<client_state::DrivenInput>();
    app.update();
    assert_eq!(
        app.world()
            .resource::<PendingDeviceFrame>()
            .frame
            .as_ref()
            .unwrap()
            .keyboard_mouse
            .as_ref()
            .unwrap()
            .mouse_motion,
        [12.0, -4.0]
    );
}

#[test]
fn focus_is_global_but_cursor_capture_only_gates_keyboard_mouse() {
    let focused_released = input_source_gates(true, false);
    assert!(!focused_released.keyboard_mouse);
    assert!(focused_released.controllers_and_touch);

    let unfocused = input_source_gates(false, true);
    assert!(!unfocused.keyboard_mouse);
    assert!(!unfocused.controllers_and_touch);
}

/// Controller and touch modes stay admitted without cursor capture; desktop input needs it.
#[test]
fn input_modes_follow_their_source_gates() {
    for mode in [InputMode::GamePad, InputMode::Touch] {
        assert!(admits_input_mode(mode, true, false));
        assert!(!admits_input_mode(mode, false, true));
    }
    assert!(!admits_input_mode(InputMode::KeyboardMouse, true, false));
    assert!(admits_input_mode(InputMode::KeyboardMouse, true, true));
}

#[test]
fn device_sampling_keeps_lowest_stable_ids_and_counts_overflow() {
    let controllers = (0..=MAX_CONTROLLERS)
        .rev()
        .map(|device_id| ControllerFrame {
            device_id: device_id as u32,
            ..ControllerFrame::default()
        });
    let touches = (0..=MAX_TOUCH_CONTACTS)
        .rev()
        .map(|contact_id| TouchContact {
            contact_id: contact_id as u64,
            activity_sequence: 0,
            position: [0.5, 0.5],
            delta: [0.0, 0.0],
            hit_id: None,
        });

    let bounded_controllers = select_lowest_by_key(controllers, MAX_CONTROLLERS, |controller| {
        u64::from(controller.device_id)
    });
    let bounded_touches =
        select_lowest_by_key(touches, MAX_TOUCH_CONTACTS, |touch| touch.contact_id);
    assert_eq!(bounded_controllers.samples.len(), MAX_CONTROLLERS);
    assert_eq!(bounded_touches.samples.len(), MAX_TOUCH_CONTACTS);
    assert_eq!(
        bounded_controllers
            .samples
            .iter()
            .map(|controller| controller.device_id)
            .collect::<Vec<_>>(),
        (0..MAX_CONTROLLERS as u32).collect::<Vec<_>>()
    );
    assert_eq!(
        bounded_touches
            .samples
            .iter()
            .map(|contact| contact.contact_id)
            .collect::<Vec<_>>(),
        (0..MAX_TOUCH_CONTACTS as u64).collect::<Vec<_>>()
    );
    assert_eq!(bounded_controllers.ignored, 1);
    assert_eq!(bounded_touches.ignored, 1);
}

#[test]
fn device_sampling_does_not_retain_raw_population_capacity() {
    let controllers = (0..MAX_CONTROLLERS * 8).map(|device_id| ControllerFrame {
        device_id: device_id as u32,
        ..ControllerFrame::default()
    });
    let touches = (0..MAX_TOUCH_CONTACTS * 8).map(|contact_id| TouchContact {
        contact_id: contact_id as u64,
        activity_sequence: 0,
        position: [0.5, 0.5],
        delta: [0.0, 0.0],
        hit_id: None,
    });

    let bounded_controllers = select_lowest_by_key(controllers, MAX_CONTROLLERS, |controller| {
        u64::from(controller.device_id)
    });
    let bounded_touches =
        select_lowest_by_key(touches, MAX_TOUCH_CONTACTS, |touch| touch.contact_id);

    assert!(bounded_controllers.samples.capacity() <= MAX_CONTROLLERS);
    assert!(bounded_touches.samples.capacity() <= MAX_TOUCH_CONTACTS);
}

#[test]
fn production_device_sampling_bounds_controller_allocation_before_translation() {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .add_systems(Update, collect_raw_input);
    app.world_mut().spawn((
        Window {
            focused: true,
            ..Window::default()
        },
        CursorOptions {
            grab_mode: CursorGrabMode::Locked,
            visible: false,
            ..CursorOptions::default()
        },
        PrimaryWindow,
    ));
    for _ in 0..MAX_CONTROLLERS * 8 {
        app.world_mut().spawn(Gamepad::default());
    }

    app.update();

    let pending = app.world().resource::<PendingDeviceFrame>();
    let frame = pending.frame.as_ref().unwrap();
    assert_eq!(frame.controllers.len(), MAX_CONTROLLERS);
    assert!(frame.controllers.capacity() <= MAX_CONTROLLERS);
    assert_eq!(pending.ignored_controllers, (MAX_CONTROLLERS * 7) as u64);
}

#[test]
fn touch_mode_blocks_keyboard_and_mouse_gameplay_input() {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<SemanticInputRuntime>()
        .init_resource::<SemanticRouteState>()
        .insert_resource(client_state::EffectiveGameplayInputMode(InputMode::Touch))
        .add_systems(Update, (collect_raw_input, route_semantic_input).chain());
    app.world_mut().spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        CursorOptions {
            grab_mode: CursorGrabMode::Locked,
            visible: false,
            ..Default::default()
        },
        PrimaryWindow,
    ));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut()
        .resource_mut::<AccumulatedMouseMotion>()
        .delta = bevy::prelude::Vec2::new(10.0, 5.0);

    app.update();

    let runtime = app.world().resource::<SemanticInputRuntime>();
    assert_eq!(runtime.preferred_input_mode(), Some(InputMode::Touch));
    let mut runtime = app.world_mut().resource_mut::<SemanticInputRuntime>();
    let snapshot = runtime.finalize_routed_input().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::Touch);
    assert_eq!(snapshot.movement, [0.0, 0.0]);
    assert!(!snapshot.phases[Action::Attack as usize].held);
}

#[test]
fn touch_mode_allows_touch_gameplay_input() {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<SemanticInputRuntime>()
        .init_resource::<SemanticRouteState>()
        .insert_resource(client_state::EffectiveGameplayInputMode(InputMode::Touch))
        .add_systems(Update, (collect_raw_input, route_semantic_input).chain());
    app.world_mut().spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        CursorOptions::default(),
        PrimaryWindow,
    ));
    app.world_mut()
        .resource_mut::<SemanticTouchTargets>()
        .pulse(TouchContact {
            contact_id: 1,
            activity_sequence: 1,
            position: [0.5, 0.5],
            delta: [0.0, 0.0],
            hit_id: Some(semantic_input::touch::JUMP),
        });

    app.update();

    let mut runtime = app.world_mut().resource_mut::<SemanticInputRuntime>();
    let snapshot = runtime.finalize_routed_input().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::Touch);
    assert!(snapshot.phases[Action::Jump as usize].pressed);
}

#[test]
fn keyboard_mouse_mode_blocks_touch_gameplay_input() {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<SemanticInputRuntime>()
        .init_resource::<SemanticRouteState>()
        .insert_resource(client_state::EffectiveGameplayInputMode(
            InputMode::KeyboardMouse,
        ))
        .add_systems(Update, (collect_raw_input, route_semantic_input).chain());
    app.world_mut().spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        CursorOptions {
            grab_mode: CursorGrabMode::Locked,
            visible: false,
            ..Default::default()
        },
        PrimaryWindow,
    ));
    app.world_mut()
        .resource_mut::<SemanticTouchTargets>()
        .set(42, semantic_input::touch::JUMP);
    app.world_mut()
        .resource_mut::<SemanticTouchTargets>()
        .pulse(TouchContact {
            contact_id: 42,
            activity_sequence: 1,
            position: [0.5, 0.5],
            delta: [0.0, 0.0],
            hit_id: Some(semantic_input::touch::JUMP),
        });

    app.update();

    assert_eq!(
        app.world().resource::<SemanticTouchTargets>().target(42),
        None
    );
    let mut runtime = app.world_mut().resource_mut::<SemanticInputRuntime>();
    let snapshot = runtime.finalize_routed_input().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::KeyboardMouse);
    assert!(!snapshot.phases[Action::Jump as usize].pressed);
    assert!(!snapshot.phases[Action::Jump as usize].held);
}

#[test]
fn keyboard_mouse_mode_allows_keyboard_and_mouse_gameplay_input() {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<SemanticInputRuntime>()
        .init_resource::<SemanticRouteState>()
        .insert_resource(client_state::EffectiveGameplayInputMode(
            InputMode::KeyboardMouse,
        ))
        .add_systems(Update, (collect_raw_input, route_semantic_input).chain());
    app.world_mut().spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        CursorOptions {
            grab_mode: CursorGrabMode::Locked,
            visible: false,
            ..Default::default()
        },
        PrimaryWindow,
    ));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);

    app.update();

    let mut runtime = app.world_mut().resource_mut::<SemanticInputRuntime>();
    let snapshot = runtime.finalize_routed_input().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::KeyboardMouse);
    assert!(snapshot.phases[Action::Jump as usize].pressed);
    assert!(snapshot.phases[Action::Jump as usize].held);
}

#[test]
fn gamepad_remains_available_in_both_touch_and_keyboard_mouse_modes() {
    for mode in [InputMode::Touch, InputMode::KeyboardMouse] {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<Touches>()
            .init_resource::<SemanticTouchTargets>()
            .init_resource::<PendingDeviceFrame>()
            .init_resource::<SemanticInputRuntime>()
            .init_resource::<SemanticRouteState>()
            .insert_resource(client_state::EffectiveGameplayInputMode(mode))
            .add_systems(Update, (collect_raw_input, route_semantic_input).chain());
        app.world_mut().spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ));
        let mut pad = Gamepad::default();
        pad.digital_mut().press(GamepadButton::South);
        app.world_mut().spawn(pad);

        app.update();

        let mut runtime = app.world_mut().resource_mut::<SemanticInputRuntime>();
        let snapshot = runtime.finalize_routed_input().unwrap();
        assert_eq!(snapshot.input_mode, InputMode::GamePad);
        assert!(snapshot.phases[Action::Jump as usize].pressed);
    }
}

#[test]
fn clearing_preference_with_none_restores_previous_behaviour() {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<SemanticInputRuntime>()
        .init_resource::<SemanticRouteState>()
        .insert_resource(client_state::EffectiveGameplayInputMode(InputMode::Touch))
        .add_systems(Update, (collect_raw_input, route_semantic_input).chain());
    app.world_mut().spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        CursorOptions {
            grab_mode: CursorGrabMode::Locked,
            visible: false,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    app.update();
    let mut runtime = app.world_mut().resource_mut::<SemanticInputRuntime>();
    let snapshot = runtime.finalize_routed_input().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::Touch);

    // Clear effective mode
    app.world_mut()
        .remove_resource::<client_state::EffectiveGameplayInputMode>();

    // Press keyboard key
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    app.update();

    let mut runtime = app.world_mut().resource_mut::<SemanticInputRuntime>();
    let snapshot = runtime.finalize_routed_input().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::KeyboardMouse);
    assert!(snapshot.phases[Action::Jump as usize].held);

    // Release key; unconstrained fallback keeps KeyboardMouse
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::Space);
    app.update();

    let mut runtime = app.world_mut().resource_mut::<SemanticInputRuntime>();
    let snapshot = runtime.finalize_routed_input().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::KeyboardMouse);
    assert!(!snapshot.phases[Action::Jump as usize].held);
}

#[test]
fn switching_modes_does_not_leave_stale_touch_or_physical_input_state() {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<SemanticInputRuntime>()
        .init_resource::<SemanticRouteState>()
        .insert_resource(client_state::EffectiveGameplayInputMode(InputMode::Touch))
        .add_systems(Update, (collect_raw_input, route_semantic_input).chain());
    app.world_mut().spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        CursorOptions {
            grab_mode: CursorGrabMode::Locked,
            visible: false,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    // Active touch jump in Touch mode
    app.world_mut()
        .resource_mut::<SemanticTouchTargets>()
        .set(10, semantic_input::touch::JUMP);
    app.world_mut()
        .resource_mut::<SemanticTouchTargets>()
        .pulse(TouchContact {
            contact_id: 10,
            activity_sequence: 1,
            position: [0.5, 0.5],
            delta: [0.0, 0.0],
            hit_id: Some(semantic_input::touch::JUMP),
        });

    app.update();
    let mut runtime = app.world_mut().resource_mut::<SemanticInputRuntime>();
    let snapshot = runtime.finalize_routed_input().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::Touch);
    assert!(snapshot.phases[Action::Jump as usize].pressed);

    // Switch to Keyboard & Mouse mode
    app.world_mut()
        .insert_resource(client_state::EffectiveGameplayInputMode(
            InputMode::KeyboardMouse,
        ));

    app.update();
    assert_eq!(
        app.world().resource::<SemanticTouchTargets>().target(10),
        None
    );
    let mut runtime = app.world_mut().resource_mut::<SemanticInputRuntime>();
    let snapshot = runtime.finalize_routed_input().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::KeyboardMouse);
    assert!(!snapshot.phases[Action::Jump as usize].pressed);
    assert!(!snapshot.phases[Action::Jump as usize].held);

    // Press keyboard space in Keyboard & Mouse mode
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    app.update();
    let mut runtime = app.world_mut().resource_mut::<SemanticInputRuntime>();
    let snapshot = runtime.finalize_routed_input().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::KeyboardMouse);
    assert!(snapshot.phases[Action::Jump as usize].held);

    // Switch back to Touch mode while space is still physically held
    app.world_mut()
        .insert_resource(client_state::EffectiveGameplayInputMode(InputMode::Touch));

    app.update();
    let mut runtime = app.world_mut().resource_mut::<SemanticInputRuntime>();
    let snapshot = runtime.finalize_routed_input().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::Touch);
    assert!(!snapshot.phases[Action::Jump as usize].held);
}

#[test]
fn desktop_behaviour_remains_unchanged() {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<SemanticInputRuntime>()
        .init_resource::<SemanticRouteState>()
        .add_systems(Update, (collect_raw_input, route_semantic_input).chain());
    app.world_mut().spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        CursorOptions {
            grab_mode: CursorGrabMode::Locked,
            visible: false,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    // Keyboard space produces KeyboardMouse mode and Jump action
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    app.update();
    let mut runtime = app.world_mut().resource_mut::<SemanticInputRuntime>();
    let snapshot = runtime.finalize_routed_input().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::KeyboardMouse);
    assert!(snapshot.phases[Action::Jump as usize].held);

    // Touch contact routes as Touch
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::Space);
    app.world_mut()
        .resource_mut::<SemanticTouchTargets>()
        .pulse(TouchContact {
            contact_id: 2,
            activity_sequence: 10,
            position: [0.5, 0.5],
            delta: [0.0, 0.0],
            hit_id: Some(semantic_input::touch::JUMP),
        });
    app.update();
    let mut runtime = app.world_mut().resource_mut::<SemanticInputRuntime>();
    let snapshot = runtime.finalize_routed_input().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::Touch);
}

#[test]
fn keyboard_mouse_mode_preserves_raw_touches_for_menu() {
    use bevy::input::touch::{TouchInput, TouchPhase, touch_screen_input_system};

    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .add_message::<TouchInput>()
        .init_resource::<Touches>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .insert_resource(client_state::EffectiveGameplayInputMode(
            InputMode::KeyboardMouse,
        ))
        .add_systems(PreUpdate, touch_screen_input_system)
        .add_systems(Update, collect_raw_input);
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

    app.world_mut().write_message(TouchInput {
        phase: TouchPhase::Started,
        position: bevy::prelude::Vec2::new(100.0, 100.0),
        force: None,
        id: 42,
        window,
    });

    app.update();

    let touches = app.world().resource::<Touches>();
    assert!(touches.get_pressed(42).is_some());

    let pending = app.world().resource::<PendingDeviceFrame>();
    assert!(pending.frame.as_ref().unwrap().touches.is_empty());
}
