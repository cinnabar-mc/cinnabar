//! Platform-level physical input device tracking.

use bevy::{
    ecs::entity::Entity,
    input::{
        ButtonState,
        mouse::{AccumulatedMouseMotion, MouseButtonInput, MouseScrollUnit, MouseWheel},
    },
    math::Vec2,
    prelude::{Messages, Query, ResMut, With},
    window::{PrimaryWindow, WindowEvent},
};
pub use client_state::ConnectedPhysicalDevices;
pub use client_state::devices::{DeviceTracker, RawInputDeviceInfo};

/// Synchronizes the latest atomic platform-reported device state into the ECS resource.
pub fn sync_connected_physical_devices(mut devices: ResMut<ConnectedPhysicalDevices>) {
    let (mice, keyboards) = client_state::platform_physical_devices();
    if devices.mice != mice || devices.keyboards != keyboards {
        devices.mice = mice;
        devices.keyboards = keyboards;
    }
}

/// Drains platform-queued mouse motion, clicks and scroll wheel events into Bevy inputs.
pub fn drain_platform_mouse_events(
    mut motion: ResMut<AccumulatedMouseMotion>,
    mut button_messages: ResMut<Messages<MouseButtonInput>>,
    mut window_messages: Option<ResMut<Messages<WindowEvent>>>,
    mut wheel_messages: ResMut<Messages<MouseWheel>>,
    windows: Query<Entity, With<PrimaryWindow>>,
) {
    let window = windows.iter().next().unwrap_or(Entity::PLACEHOLDER);

    // 1. Drain relative mouse motion:
    let (dx, dy) = client_state::devices::take_platform_mouse_motion();
    if dx != 0.0 || dy != 0.0 {
        motion.delta += Vec2::new(dx, dy);
    }

    // 2. Drain button clicks:
    let buttons = client_state::devices::take_platform_mouse_buttons();
    for event in buttons {
        if let Some(button) =
            client_state::devices::android_button_to_mouse_button(event.button_mask)
        {
            let state = if event.pressed {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            };
            let input_event = MouseButtonInput {
                button,
                state,
                window,
            };
            button_messages.write(input_event);
            if let Some(ref mut window_events) = window_messages {
                window_events.write(WindowEvent::MouseButtonInput(input_event));
            }
        }
    }

    // 3. Drain scroll wheel:
    let scrolls = client_state::devices::take_platform_mouse_scrolls();
    for event in scrolls {
        wheel_messages.write(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: event.hscroll,
            y: event.vscroll,
            window,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
    }
}

/// Helper called by platform ingress (e.g. Android JNI) to update connected devices.
pub fn set_platform_physical_devices(mice: u32, keyboards: u32) {
    client_state::set_platform_physical_devices(mice, keyboards);
}

/// Adds relative mouse movement from the platform (e.g. Android JNI).
pub fn accumulate_platform_mouse_motion(dx: f32, dy: f32) {
    client_state::devices::accumulate_platform_mouse_motion(dx, dy);
}

/// Enqueues a mouse button press or release from the platform.
pub fn push_platform_mouse_button(button_mask: u32, pressed: bool) {
    client_state::devices::push_platform_mouse_button(button_mask, pressed);
}

/// Enqueues a mouse scroll event from the platform.
pub fn push_platform_mouse_scroll(vscroll: f32, hscroll: f32) {
    client_state::devices::push_platform_mouse_scroll(vscroll, hscroll);
}

/// Sets whether the OS pointer capture is currently active.
pub fn set_platform_pointer_captured(captured: bool) {
    client_state::devices::set_platform_pointer_captured(captured);
}

/// Reads whether the OS pointer capture is currently active.
pub fn is_platform_pointer_captured() -> bool {
    client_state::devices::is_platform_pointer_captured()
}

/// Registers the platform function that requests or releases OS pointer capture.
pub fn set_capture_driver(driver: fn(bool)) {
    client_state::devices::set_capture_driver(driver);
}

/// Requests or releases OS pointer capture through the registered platform driver.
pub fn request_platform_pointer_capture(enable: bool) {
    client_state::devices::request_platform_pointer_capture(enable);
}
