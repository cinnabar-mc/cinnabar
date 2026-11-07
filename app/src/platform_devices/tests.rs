use bevy::{
    ecs::system::RunSystemOnce,
    input::{
        ButtonInput,
        keyboard::KeyCode,
        mouse::{AccumulatedMouseMotion, MouseButton},
        touch::{TouchInput, TouchPhase},
    },
    math::Vec2,
    prelude::App,
};
use client_shell::menu::MenuRuntime;
use client_state::{
    ConnectedPhysicalDevices, DetectedGameplayInputMode, EffectiveGameplayInputMode, ScreenFocus,
};
use semantic_input::InputMode;

use super::*;
use crate::ui_runtime::touch_pointer::UiTouchStarts;

fn setup_test_app() -> App {
    let mut app = App::new();
    app.add_plugins(bevy::time::TimePlugin);
    app.init_resource::<ConnectedPhysicalDevices>();
    app.init_resource::<DetectedGameplayInputMode>();
    app.init_resource::<EffectiveGameplayInputMode>();
    app.init_resource::<AccumulatedMouseMotion>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.init_resource::<ButtonInput<MouseButton>>();
    app.init_resource::<UiTouchStarts>();
    app.init_resource::<ScreenFocus>();
    app.add_message::<TouchInput>();
    app
}

#[test]
fn physical_mouse_arrival_overrules_gameplay_touch_in_same_frame() {
    let mut app = setup_test_app();
    // Simulate initial touch mode with 0 devices
    app.world_mut()
        .resource_mut::<DetectedGameplayInputMode>()
        .0 = InputMode::Touch;
    app.world_mut()
        .resource_mut::<EffectiveGameplayInputMode>()
        .0 = InputMode::Touch;

    // In the same frame: mouse arrives AND a gameplay touch start is registered
    app.world_mut()
        .resource_mut::<ConnectedPhysicalDevices>()
        .mice = 1;
    app.world_mut().write_message(TouchInput {
        phase: TouchPhase::Started,
        position: Vec2::new(100.0, 100.0),
        force: None,
        id: 1,
        window: bevy::ecs::entity::Entity::PLACEHOLDER,
    });

    app.world_mut()
        .run_system_once(arbitrate_gameplay_input_mode)
        .unwrap();

    let effective = app.world().resource::<EffectiveGameplayInputMode>().0;
    assert_eq!(
        effective,
        InputMode::KeyboardMouse,
        "Physical mouse connection must take precedence over simultaneous gameplay touch"
    );
}

#[test]
fn physical_keyboard_key_overrules_gameplay_touch_in_same_frame() {
    let mut app = setup_test_app();
    app.world_mut()
        .resource_mut::<ConnectedPhysicalDevices>()
        .keyboards = 1;
    app.world_mut()
        .resource_mut::<DetectedGameplayInputMode>()
        .0 = InputMode::Touch;
    app.world_mut()
        .resource_mut::<EffectiveGameplayInputMode>()
        .0 = InputMode::Touch;

    // First frame settles previous counts
    app.world_mut()
        .run_system_once(arbitrate_gameplay_input_mode)
        .unwrap();
    // Reset to Touch to test same-frame activity priority
    app.world_mut()
        .resource_mut::<EffectiveGameplayInputMode>()
        .0 = InputMode::Touch;

    // In the same frame: physical key pressed AND gameplay touch started
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    app.world_mut().write_message(TouchInput {
        phase: TouchPhase::Started,
        position: Vec2::new(100.0, 100.0),
        force: None,
        id: 2,
        window: bevy::ecs::entity::Entity::PLACEHOLDER,
    });

    app.world_mut()
        .run_system_once(arbitrate_gameplay_input_mode)
        .unwrap();

    let effective = app.world().resource::<EffectiveGameplayInputMode>().0;
    assert_eq!(
        effective,
        InputMode::KeyboardMouse,
        "Physical key press must take precedence over simultaneous gameplay touch"
    );
}

#[test]
fn physical_mouse_motion_overrules_gameplay_touch_in_same_frame() {
    let mut app = setup_test_app();
    app.world_mut()
        .resource_mut::<ConnectedPhysicalDevices>()
        .mice = 1;
    app.world_mut()
        .resource_mut::<DetectedGameplayInputMode>()
        .0 = InputMode::Touch;
    app.world_mut()
        .resource_mut::<EffectiveGameplayInputMode>()
        .0 = InputMode::Touch;

    // Settle device count
    app.world_mut()
        .run_system_once(arbitrate_gameplay_input_mode)
        .unwrap();
    app.world_mut()
        .resource_mut::<EffectiveGameplayInputMode>()
        .0 = InputMode::Touch;

    // Mouse movement + touch start in same frame
    app.world_mut()
        .resource_mut::<AccumulatedMouseMotion>()
        .delta = Vec2::new(5.0, -3.0);
    app.world_mut().write_message(TouchInput {
        phase: TouchPhase::Started,
        position: Vec2::new(100.0, 100.0),
        force: None,
        id: 3,
        window: bevy::ecs::entity::Entity::PLACEHOLDER,
    });

    app.world_mut()
        .run_system_once(arbitrate_gameplay_input_mode)
        .unwrap();

    let effective = app.world().resource::<EffectiveGameplayInputMode>().0;
    assert_eq!(
        effective,
        InputMode::KeyboardMouse,
        "Physical mouse motion must take precedence over simultaneous gameplay touch"
    );
}

#[test]
fn touch_on_menu_does_not_switch_mode_even_if_screen_focus_is_stale() {
    let mut app = setup_test_app();
    app.world_mut()
        .resource_mut::<ConnectedPhysicalDevices>()
        .mice = 1;
    app.world_mut()
        .resource_mut::<DetectedGameplayInputMode>()
        .0 = InputMode::KeyboardMouse;
    app.world_mut()
        .resource_mut::<EffectiveGameplayInputMode>()
        .0 = InputMode::KeyboardMouse;

    // Set up live MenuRuntime with a visible menu (visible: true)
    let menu = MenuRuntime::new(true, 1, "Test".into());
    assert!(menu.is_visible());
    app.insert_resource(menu);

    // Stale ScreenFocus says gameplay is active (menu_visible: false, absorbs_input: false)
    app.world_mut().resource_mut::<ScreenFocus>().menu_visible = false;
    app.world_mut().resource_mut::<ScreenFocus>().absorbs_input = false;

    // Touch start arrives
    app.world_mut().write_message(TouchInput {
        phase: TouchPhase::Started,
        position: Vec2::new(200.0, 200.0),
        force: None,
        id: 4,
        window: bevy::ecs::entity::Entity::PLACEHOLDER,
    });

    app.world_mut()
        .run_system_once(arbitrate_gameplay_input_mode)
        .unwrap();

    let effective = app.world().resource::<EffectiveGameplayInputMode>().0;
    assert_eq!(
        effective,
        InputMode::KeyboardMouse,
        "Touch on live visible menu must NOT switch mode to Touch even if ScreenFocus resource is stale"
    );
}

#[test]
fn touch_claimed_by_ui_touch_starts_never_switches_mode() {
    let mut app = setup_test_app();
    app.world_mut()
        .resource_mut::<ConnectedPhysicalDevices>()
        .mice = 1;
    app.world_mut()
        .resource_mut::<DetectedGameplayInputMode>()
        .0 = InputMode::KeyboardMouse;
    app.world_mut()
        .resource_mut::<EffectiveGameplayInputMode>()
        .0 = InputMode::KeyboardMouse;

    // UI touch starts claims contact id 5
    app.world_mut().resource_mut::<UiTouchStarts>().0.push(5);

    app.world_mut().write_message(TouchInput {
        phase: TouchPhase::Started,
        position: Vec2::new(50.0, 50.0),
        force: None,
        id: 5,
        window: bevy::ecs::entity::Entity::PLACEHOLDER,
    });

    app.world_mut()
        .run_system_once(arbitrate_gameplay_input_mode)
        .unwrap();

    let effective = app.world().resource::<EffectiveGameplayInputMode>().0;
    assert_eq!(
        effective,
        InputMode::KeyboardMouse,
        "Touch claimed by UiTouchStarts must never switch mode to Touch"
    );
}

#[test]
fn gameplay_touch_switches_to_touch_when_no_higher_priority_physical_input() {
    let mut app = setup_test_app();
    let sys_id = app.register_system(arbitrate_gameplay_input_mode);

    app.world_mut()
        .resource_mut::<ConnectedPhysicalDevices>()
        .mice = 1;
    // Settle device count in KeyboardMouse
    app.world_mut().run_system(sys_id).unwrap();
    assert_eq!(
        app.world().resource::<EffectiveGameplayInputMode>().0,
        InputMode::KeyboardMouse
    );
    app.update();

    // Unclaimed touch start in active gameplay with no simultaneous physical input
    app.world_mut().write_message(TouchInput {
        phase: TouchPhase::Started,
        position: Vec2::new(300.0, 300.0),
        force: None,
        id: 10,
        window: bevy::ecs::entity::Entity::PLACEHOLDER,
    });

    app.world_mut().run_system(sys_id).unwrap();

    let effective = app.world().resource::<EffectiveGameplayInputMode>().0;
    assert_eq!(
        effective,
        InputMode::Touch,
        "Valid gameplay touch when no higher priority physical input exists must switch to Touch"
    );
}
