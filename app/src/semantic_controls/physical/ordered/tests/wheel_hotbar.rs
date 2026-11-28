//! Ordering regression: a backend wheel must reach gameplay hotbar selection only under the
//! player's wheel setting and the shared notch/cooldown policy.

use std::time::Duration;

use bevy::{
    ecs::system::RunSystemOnce,
    input::mouse::{MouseScrollUnit, MouseWheel},
    time::{Real, Time},
};
use client_interaction::{
    hotbar::{PIXELS_PER_NOTCH, WheelAccumulator},
    item_use::ItemUseRuntime,
};
use client_state::{PlayerRuntime, RuntimeSettings, ScreenFocus, SemanticInputSnapshot};
use semantic_input::{Action, SemanticInputRouter};

use super::*;

#[derive(bevy::prelude::Resource)]
struct TestRouter(SemanticInputRouter);

/// Builds the smallest app that runs a real wheel event through collection, routing and the
/// production ordered-selection system, without a renderer or a server.
fn wheel_app(wheel_enabled: bool) -> App {
    let mut settings = ui::UserSettings::default();
    settings.gameplay.mouse_wheel_hotbar_scrolling = wheel_enabled;
    let mut runtime = RuntimeSettings::default();
    runtime.replace_user_settings(settings);
    let mut player = PlayerRuntime(player_state::PlayerState::new(1));
    player.inventory.set_local_selected_slot(0);

    let mut app = App::new();
    app.add_message::<WindowEvent>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<bevy::input::touch::Touches>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<WheelAccumulator>()
        .init_resource::<ItemUseRuntime>()
        .init_resource::<Time<Real>>()
        .insert_resource(runtime)
        .insert_resource(TestRouter(SemanticInputRouter::default()))
        .insert_resource(ScreenFocus::default())
        .insert_resource(player)
        .add_systems(Update, collect_raw_input);
    app.world_mut().spawn((
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
    ));
    app
}

/// Delivers one real `WindowEvent::MouseWheel` and reports the signed hotbar steps that reached
/// the production selection system. The router starts in `Gameplay`, its default context.
fn ordered_wheel_frame(app: &mut App, unit: MouseScrollUnit, delta_y: f32) -> Vec<Action> {
    let window = {
        let mut windows = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>();
        windows.single(app.world()).unwrap()
    };
    app.world_mut().write_message(WindowEvent::MouseWheel(MouseWheel {
        phase: bevy::input::touch::TouchPhase::Moved,
        unit,
        x: 0.0,
        y: delta_y,
        window,
    }));
    app.update();
    let mut frame = app
        .world_mut()
        .resource_mut::<PendingDeviceFrame>()
        .frame
        .take()
        .expect("collected device frame");
    if let Some(keyboard) = frame.keyboard_mouse.as_mut() {
        keyboard.activity_sequence = 1;
    }
    let snapshot = {
        let router = &mut app.world_mut().resource_mut::<TestRouter>().0;
        router.route(frame).expect("gameplay frame routes");
        router.finalize().expect("gameplay frame finalizes")
    };
    assert!(snapshot.ordered.is_some(), "ordered gameplay actions exist");
    let mut admitted = Vec::new();
    for edge in snapshot.ordered.unwrap().events() {
        if edge.pressed
            && matches!(edge.action, Action::HotbarNext | Action::HotbarPrevious)
        {
            app.world_mut()
                .run_system_cached_with(crate::hotbar::ordered::select, edge.action)
                .expect("ordered hotbar selection runs");
            admitted.push(edge.action);
        }
    }
    admitted
}

#[test]
fn opt_out_silences_two_rapid_ordered_wheel_events() {
    let mut app = wheel_app(false);
    assert_eq!(ordered_wheel_frame(&mut app, MouseScrollUnit::Line, -3.0), Vec::new());
    assert_eq!(ordered_wheel_frame(&mut app, MouseScrollUnit::Line, -3.0), Vec::new());
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(0),
        "the wheel must not select a slot while the setting is off"
    );
}

#[test]
fn one_line_notch_selects_exactly_one_slot_and_cooldown_limits_a_rapid_repeat() {
    let mut app = wheel_app(true);
    let notch = -3.0;
    assert_eq!(
        ordered_wheel_frame(&mut app, MouseScrollUnit::Line, notch),
        vec![Action::HotbarNext],
        "one physical line notch selects the next slot with the setting on"
    );
    assert_eq!(
        ordered_wheel_frame(&mut app, MouseScrollUnit::Line, notch),
        Vec::new(),
        "a second line notch inside the cooldown must not select another slot"
    );
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(1)
    );
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(100));
    assert_eq!(
        ordered_wheel_frame(&mut app, MouseScrollUnit::Line, notch),
        vec![Action::HotbarNext],
        "a fresh notch after the cooldown may step again"
    );
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(2)
    );
}

/// Control: pixel deltas keep the 16 px per notch policy; a sub-notch pixel drift stays silent.
#[test]
fn ordered_pixel_delta_still_uses_sixteen_pixels_per_notch() {
    let mut app = wheel_app(true);
    assert_eq!(
        ordered_wheel_frame(&mut app, MouseScrollUnit::Pixel, -PIXELS_PER_NOTCH),
        vec![Action::HotbarNext]
    );
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(1)
    );
    let mut drifting = wheel_app(true);
    assert_eq!(
        ordered_wheel_frame(&mut drifting, MouseScrollUnit::Pixel, -7.0),
        Vec::new(),
        "half a pixel-notch is below the shared threshold"
    );
}

/// Touch frames never publish ordered wheel actions: the raw hotbar path must be the only
/// accumulator consumer, so one physical delta cannot be counted twice.
#[test]
fn touch_mode_wheel_is_consumed_only_by_the_raw_path() {
    let mut app = wheel_app(true);
    app.insert_resource(client_state::EffectiveGameplayInputMode(
        semantic_input::InputMode::Touch,
    ))
    .insert_resource(SemanticInputSnapshot::default())
    .insert_resource(bevy::input::mouse::AccumulatedMouseScroll {
        delta: bevy::prelude::Vec2::new(0.0, -6.0),
        unit: MouseScrollUnit::Pixel,
        ..Default::default()
    });
    let window = {
        let mut windows = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>();
        windows.single(app.world()).unwrap()
    };
    app.world_mut()
        .write_message(WindowEvent::MouseWheel(MouseWheel {
            phase: bevy::input::touch::TouchPhase::Moved,
            unit: MouseScrollUnit::Pixel,
            x: 0.0,
            y: -6.0,
            window,
        }));
    app.update();
    app.world_mut()
        .run_system_once(client_interaction::hotbar::queue_hotbar_selection)
        .expect("raw hotbar selection runs");
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(0),
        "ordered collection must not consume a wheel delta it cannot publish"
    );
    app.world_mut()
        .run_system_once(client_interaction::hotbar::queue_hotbar_selection)
        .expect("raw hotbar selection runs again");
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(1),
        "the second raw sub-threshold delta completes one notch"
    );
}

/// Control: a digit key is still admitted through the same ordered dispatcher with the wheel off.
#[test]
fn opt_out_keeps_ordered_digit_selection() {
    let mut app = wheel_app(false);
    let window = {
        let mut windows = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>();
        windows.single(app.world()).unwrap()
    };
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(WindowEvent::KeyboardInput(
            bevy::input::keyboard::KeyboardInput {
                key_code: KeyCode::Digit2,
                logical_key: bevy::input::keyboard::Key::Character("2".into()),
                state,
                text: None,
                repeat: false,
                window,
            },
        ));
    }
    app.update();
    let mut frame = app
        .world_mut()
        .resource_mut::<PendingDeviceFrame>()
        .frame
        .take()
        .unwrap();
    if let Some(keyboard) = frame.keyboard_mouse.as_mut() {
        keyboard.activity_sequence = 1;
    }
    let snapshot = {
        let router = &mut app.world_mut().resource_mut::<TestRouter>().0;
        router.route(frame).unwrap();
        router.finalize().unwrap()
    };
    for edge in snapshot.ordered.unwrap().events() {
        if edge.pressed {
            app.world_mut()
                .run_system_cached_with(crate::hotbar::ordered::select, edge.action)
                .unwrap();
        }
    }
    assert_eq!(
        app.world()
            .resource::<PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(1),
        "the Hotbar2 digit selects slot index 1"
    );
}

/// Control: `HotbarNext` from a non-wheel source is unaffected by the wheel setting.
#[test]
fn opt_out_keeps_hotbar_next_from_other_sources() {
    let settings = semantic_input::ControlSettings::default();
    let mut bindings = settings.bindings().to_vec();
    bindings.push(semantic_input::ActionBinding {
        action: Action::HotbarNext,
        context: semantic_input::InputContext::Gameplay,
        chord: semantic_input::InputChord {
            control: semantic_input::PhysicalControl::KeyboardUsage(0x2e),
            modifiers: semantic_input::ModifierChord::default(),
        },
    });
    let settings = semantic_input::ControlSettings::new(
        bindings,
        semantic_input::DEFAULT_MOUSE_SENSITIVITY,
        1.0,
        1.0,
        false,
        false,
        0.15,
        0.15,
    )
    .unwrap();
    let mut router = SemanticInputRouter::default();
    router.replace_bindings(settings).unwrap();
    // A binding replacement releases held input; settle that authority change first so the next
    // frame's ordered edges are admitted.
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    router.finalize().unwrap();
    let mut events = semantic_input::DesktopEvents::default();
    events.push(semantic_input::DesktopEvent::Button {
        control: semantic_input::PhysicalControl::KeyboardUsage(0x2e),
        pressed: true,
        modifiers: semantic_input::ModifierChord::default(),
    });
    router
        .route(DeviceFrame {
            desktop_events: Some(events),
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    let snapshot = router.finalize().unwrap();
    let edges: Vec<_> = snapshot
        .ordered
        .unwrap()
        .events()
        .iter()
        .filter(|edge| edge.pressed)
        .map(|edge| edge.action)
        .collect();
    assert!(edges.contains(&Action::HotbarNext), "non-wheel next survives");
}
