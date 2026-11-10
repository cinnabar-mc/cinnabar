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

    // Platform mouse movement + touch start in same frame
    app.insert_resource(PlatformMouseActivity(true));
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

/// Platform mouse input reaches the Bevy input state that gameplay reads after `InputSystems`
/// rebuild it for the frame.
#[test]
fn platform_mouse_input_survives_bevy_input_systems() {
    let mut app = App::new();
    app.add_plugins((bevy::prelude::MinimalPlugins, bevy::input::InputPlugin))
        .add_systems(
            bevy::prelude::PreUpdate,
            drain_platform_mouse_events.before(bevy::input::InputSystems),
        )
        .init_resource::<PlatformMouseActivity>();
    let _globals = ingress::lock_test_globals();
    let _ = ingress::take_platform_mouse_motion();
    let _ = ingress::take_platform_mouse_events();
    ingress::accumulate_platform_mouse_motion(3.0, -2.0);
    ingress::push_platform_mouse_button(ingress::mouse_buttons::BUTTON_PRIMARY, true, 0);

    app.update();

    assert_eq!(
        app.world().resource::<AccumulatedMouseMotion>().delta,
        Vec2::new(3.0, -2.0)
    );
    assert!(
        app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left)
    );
    assert!(app.world().resource::<PlatformMouseActivity>().0);
}

fn gameplay_touch_start(id: u64) -> TouchInput {
    TouchInput {
        phase: TouchPhase::Started,
        position: Vec2::new(100.0, 100.0),
        force: None,
        id,
        window: bevy::ecs::entity::Entity::PLACEHOLDER,
    }
}

/// An app that runs arbitration once per `update`, keeping its device counts and message cursor
/// across frames like production, starting in Touch with devices already settled.
fn scheduled_arbitration_app(mice: u32, keyboards: u32) -> App {
    let mut app = setup_test_app();
    app.add_systems(bevy::prelude::Update, arbitrate_gameplay_input_mode);
    *app.world_mut().resource_mut::<ConnectedPhysicalDevices>() =
        ConnectedPhysicalDevices { mice, keyboards };
    app.update();
    app.world_mut()
        .resource_mut::<EffectiveGameplayInputMode>()
        .0 = InputMode::Touch;
    app
}

/// A touch start that loses to a same-frame key press is consumed, so it cannot switch back to
/// Touch on the next frame.
#[test]
fn touch_start_losing_to_physical_input_does_not_switch_later() {
    let mut app = scheduled_arbitration_app(0, 1);

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    app.world_mut().write_message(gameplay_touch_start(4));
    app.update();
    assert_eq!(
        app.world().resource::<EffectiveGameplayInputMode>().0,
        InputMode::KeyboardMouse
    );

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.update();
    assert_eq!(
        app.world().resource::<EffectiveGameplayInputMode>().0,
        InputMode::KeyboardMouse
    );
}

/// A touch-emulated UI pointer press is not physical mouse activity, even with a mouse connected.
#[test]
fn touch_emulated_mouse_press_keeps_touch_mode() {
    let mut app = scheduled_arbitration_app(1, 0);

    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();

    assert_eq!(
        app.world().resource::<EffectiveGameplayInputMode>().0,
        InputMode::Touch
    );
}

/// Gameplay touch still selects Touch after physical input, with devices still connected.
#[test]
fn gameplay_touch_after_physical_input_selects_touch() {
    let mut app = scheduled_arbitration_app(1, 1);
    app.insert_resource(PlatformMouseActivity(true));
    app.update();
    assert_eq!(
        app.world().resource::<EffectiveGameplayInputMode>().0,
        InputMode::KeyboardMouse
    );

    app.insert_resource(PlatformMouseActivity(false));
    app.world_mut().write_message(gameplay_touch_start(5));
    app.update();
    assert_eq!(
        app.world().resource::<EffectiveGameplayInputMode>().0,
        InputMode::Touch
    );
}

/// The touch that switches back to Touch finds the touch controls already shown, so gameplay
/// touch sampling later in the same frame can capture it.
#[test]
fn switching_to_touch_shows_touch_controls_before_gameplay_sampling() {
    let mut app = scheduled_arbitration_app(1, 0);
    app.world_mut()
        .resource_mut::<EffectiveGameplayInputMode>()
        .0 = InputMode::KeyboardMouse;
    let mut menu = MenuRuntime::new(false, 1, "Test".into());
    menu.set_touch_available(true);
    let mut presentation = client_ui::ui_runtime::presentation::UiPresentationRuntime::new(
        ui_engine::test_support::fixture_font(),
    )
    .unwrap();
    presentation.enable_touch_controls(false);
    app.insert_resource(menu)
        .insert_resource(UiPresentationResource(presentation));

    app.world_mut().write_message(gameplay_touch_start(6));
    app.update();

    assert_eq!(
        app.world().resource::<EffectiveGameplayInputMode>().0,
        InputMode::Touch
    );
    assert!(
        app.world()
            .resource::<UiPresentationResource>()
            .touch_controls_enabled()
    );
}

/// Platform cursor positions, buttons and wheel steps reach the ordered window events in arrival
/// order, so UI clicks hit where the cursor is and wheel actions such as hotbar scrolling see
/// them; uncaptured movement also counts as mouse activity.
#[test]
fn platform_mouse_events_reach_ordered_window_events_in_order() {
    let _globals = ingress::lock_test_globals();
    let _ = ingress::take_platform_mouse_motion();
    let _ = ingress::take_platform_mouse_events();
    let mut app = App::new();
    app.add_message::<bevy::input::mouse::MouseMotion>()
        .add_message::<MouseButtonInput>()
        .add_message::<MouseWheel>()
        .add_message::<CursorMoved>()
        .add_message::<WindowEvent>()
        .init_resource::<PlatformMouseActivity>()
        .init_resource::<PlatformButtonModifiers>();
    let mut window = Window::default();
    window.resolution.set_scale_factor(2.0);
    app.world_mut().spawn((window, PrimaryWindow));

    ingress::push_platform_mouse_cursor(80.0, 60.0);
    ingress::push_platform_mouse_button(
        ingress::mouse_buttons::BUTTON_PRIMARY,
        true,
        ingress::android_meta::META_SHIFT_ON,
    );
    ingress::push_platform_mouse_scroll(-1.0, 0.0);
    app.world_mut()
        .run_system_once(drain_platform_mouse_events)
        .unwrap();

    let events = app.world().resource::<Messages<WindowEvent>>();
    let kinds = events
        .iter_current_update_messages()
        .map(|event| match event {
            WindowEvent::CursorMoved(moved) if moved.position == Vec2::new(40.0, 30.0) => "cursor",
            WindowEvent::MouseButtonInput(_) => "button",
            WindowEvent::MouseWheel(wheel) if wheel.y == -1.0 => "wheel",
            _ => "other",
        })
        .collect::<Vec<_>>();
    assert_eq!(kinds, ["cursor", "button", "wheel"]);
    let mut windows = app
        .world_mut()
        .query_filtered::<&Window, With<PrimaryWindow>>();
    assert_eq!(
        windows.single(app.world()).unwrap().cursor_position(),
        Some(Vec2::new(40.0, 30.0))
    );
    assert!(app.world().resource::<PlatformMouseActivity>().0);
    let recorded = &app.world().resource::<PlatformButtonModifiers>().0;
    assert_eq!(recorded.len(), 1);
    assert!(recorded[0].1.shift);
}
