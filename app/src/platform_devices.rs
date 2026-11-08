//! Platform-level physical input device tracking.

use bevy::{
    app::App,
    ecs::entity::Entity,
    input::{
        ButtonInput, ButtonState, InputSystems,
        keyboard::KeyCode,
        mouse::{MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel},
        touch::{TouchInput, TouchPhase},
    },
    math::Vec2,
    prelude::{
        IntoScheduleConfigs, Local, MessageReader, Messages, PreUpdate, Query, Res, ResMut,
        Resource, With,
    },
    window::{PrimaryWindow, WindowEvent},
};
use client_shell::{
    menu::MenuRuntime,
    ui_resources::{UiPresentationResource, UiRuntimeResource},
};
use client_state::{
    ConnectedPhysicalDevices, DetectedGameplayInputMode, EffectiveGameplayInputMode,
};

pub mod ingress;

/// Registers physical device tracking, platform mouse ingress and automatic `Touch` /
/// `KeyboardMouse` arbitration for platforms whose edge reports physical devices (Android).
///
/// Other platforms leave the input-mode resources absent, so their keyboard, mouse and touch
/// routing is unchanged.
pub(crate) fn configure_hybrid_input(app: &mut App) {
    app.init_resource::<ConnectedPhysicalDevices>()
        .init_resource::<PlatformMouseActivity>()
        .insert_resource(DetectedGameplayInputMode(semantic_input::InputMode::Touch))
        .insert_resource(EffectiveGameplayInputMode(semantic_input::InputMode::Touch))
        .add_systems(
            PreUpdate,
            (sync_connected_physical_devices, drain_platform_mouse_events).before(InputSystems),
        )
        .add_systems(PreUpdate, arbitrate_gameplay_input_mode.after(InputSystems));
}

/// Whether the platform delivered physical mouse motion, buttons or wheel steps this frame.
///
/// Touch-emulated UI pointer presses also reach `ButtonInput<MouseButton>`, so mode arbitration
/// reads this platform-origin signal instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Resource)]
pub struct PlatformMouseActivity(pub bool);

/// Previous device counts tracked to detect physical device arrivals.
#[derive(Default)]
pub struct PreviousDeviceCounts {
    pub mice: u32,
    pub keyboards: u32,
}

/// Automatically arbitrates between `Touch` and `KeyboardMouse` modes based on
/// physical device connections, physical mouse/keyboard activity, and gameplay touch contacts.
#[allow(clippy::too_many_arguments)]
pub(crate) fn arbitrate_gameplay_input_mode(
    devices: Res<ConnectedPhysicalDevices>,
    mut prev_counts: Local<PreviousDeviceCounts>,
    mouse_activity: Option<Res<PlatformMouseActivity>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut touch_input: MessageReader<TouchInput>,
    claimed_touches: Option<Res<crate::ui_runtime::touch_pointer::UiTouchStarts>>,
    player: Option<Res<client_state::PlayerRuntime>>,
    ui: Option<Res<UiRuntimeResource>>,
    menu: Option<Res<MenuRuntime>>,
    mut presentation: Option<ResMut<UiPresentationResource>>,
    screen_focus: Option<Res<client_state::ScreenFocus>>,
    mut detected: ResMut<DetectedGameplayInputMode>,
    mut effective: ResMut<EffectiveGameplayInputMode>,
) {
    let mut target_mode = None;

    // 1. Physical mouse activity, from the platform rather than touch-emulated UI presses:
    let mouse_active = mouse_activity.is_some_and(|activity| activity.0) && devices.mice > 0;

    // 2. Physical keyboard activity. Unidentified keys include the platform Back gesture.
    let has_key_press = keys.as_ref().is_some_and(|k| {
        k.get_just_pressed()
            .any(|key| !matches!(key, KeyCode::Unidentified(_)))
    });
    let keyboard_active = has_key_press && devices.keyboards > 0;

    // 3. Device connection / disconnection transitions:
    let mice_arrived = prev_counts.mice == 0 && devices.mice > 0;
    let keyboards_arrived = prev_counts.keyboards == 0 && devices.keyboards > 0;
    let all_disconnected = devices.mice == 0 && devices.keyboards == 0;
    let disconnected_all_devices =
        all_disconnected && (prev_counts.mice > 0 || prev_counts.keyboards > 0);

    prev_counts.mice = devices.mice;
    prev_counts.keyboards = devices.keyboards;

    // Touch starts are read every frame, so a start that loses this frame's arbitration cannot
    // switch modes on a later one.
    // Evaluate live UI and menu visibility directly so arbitration never suffers
    // from single-frame lag compared to the Update-phase ScreenFocus publication.
    let gameplay_active = match (player.as_deref(), ui.as_deref(), menu.as_deref()) {
        (Some(player), Some(ui_res), menu_res) => {
            let absorbed = client_shell::screen_policy::absorbs_input(
                player,
                Some(&ui_res.0),
                menu_res,
                presentation.as_deref().map(|p| &p.0),
            );
            !absorbed
                && !ui_res.ui_focused(player)
                && !menu_res.is_some_and(MenuRuntime::is_visible)
        }
        _ => screen_focus
            .as_ref()
            .is_none_or(|focus| !focus.absorbs_input && !focus.menu_visible && !focus.ui_focused),
    };

    let mut gameplay_touch_started = false;
    for touch in touch_input.read() {
        if touch.phase == TouchPhase::Started {
            let is_ui_touch = claimed_touches
                .as_ref()
                .is_some_and(|claimed| claimed.0.contains(&touch.id));
            if !is_ui_touch && gameplay_active {
                gameplay_touch_started = true;
            }
        }
    }

    // Physical keyboard/mouse activity or connection wins, then a gameplay touch, then the last
    // physical device leaving.
    if mouse_active || keyboard_active || mice_arrived || keyboards_arrived {
        target_mode = Some(semantic_input::InputMode::KeyboardMouse);
    } else if gameplay_touch_started || disconnected_all_devices {
        target_mode = Some(semantic_input::InputMode::Touch);
    }

    if let Some(mode) = target_mode {
        detected.0 = mode;
        if effective.0 != mode {
            effective.0 = mode;
            // Show or hide touch controls now, so the touch that switched to Touch reaches
            // gameplay touch sampling later this frame.
            if let (Some(menu), Some(presentation)) = (menu.as_deref(), presentation.as_mut()) {
                client_shell::menu::screen_focus::sync_touch_controls(
                    menu,
                    Some(mode),
                    presentation,
                );
            }
        }
    }
}

/// Synchronizes the latest atomic platform-reported device state into the ECS resource.
pub fn sync_connected_physical_devices(mut devices: ResMut<ConnectedPhysicalDevices>) {
    let (mice, keyboards) = ingress::platform_physical_devices();
    if devices.mice != mice || devices.keyboards != keyboards {
        devices.mice = mice;
        devices.keyboards = keyboards;
    }
}

/// Drains platform-queued mouse motion, clicks and scroll wheel events into Bevy input messages.
///
/// Runs before `InputSystems`, which rebuild `AccumulatedMouseMotion`, `ButtonInput<MouseButton>`
/// and `AccumulatedMouseScroll` from those messages each frame.
pub fn drain_platform_mouse_events(
    mut motion_messages: ResMut<Messages<MouseMotion>>,
    mut button_messages: ResMut<Messages<MouseButtonInput>>,
    mut window_messages: Option<ResMut<Messages<WindowEvent>>>,
    mut wheel_messages: ResMut<Messages<MouseWheel>>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut activity: ResMut<PlatformMouseActivity>,
) {
    let window = windows.iter().next().unwrap_or(Entity::PLACEHOLDER);
    let mut active = false;

    // 1. Drain relative mouse motion:
    let (dx, dy) = ingress::take_platform_mouse_motion();
    if dx != 0.0 || dy != 0.0 {
        motion_messages.write(MouseMotion {
            delta: Vec2::new(dx, dy),
        });
        active = true;
    }

    // 2. Drain buttons and wheel steps in arrival order, into both the per-kind messages and the
    // ordered window events gameplay actions read.
    for event in ingress::take_platform_mouse_events() {
        match event {
            ingress::PlatformMouseEvent::Button(event) => {
                let Some(button) = ingress::android_button_to_mouse_button(event.button_mask)
                else {
                    continue;
                };
                // Releases also arrive when capture or focus is lost, so only presses show
                // activity.
                active |= event.pressed;
                let input_event = MouseButtonInput {
                    button,
                    state: if event.pressed {
                        ButtonState::Pressed
                    } else {
                        ButtonState::Released
                    },
                    window,
                };
                button_messages.write(input_event);
                if let Some(window_events) = window_messages.as_mut() {
                    window_events.write(WindowEvent::MouseButtonInput(input_event));
                }
            }
            ingress::PlatformMouseEvent::Wheel(event) => {
                let wheel = MouseWheel {
                    unit: MouseScrollUnit::Line,
                    x: event.hscroll,
                    y: event.vscroll,
                    window,
                    phase: bevy::input::touch::TouchPhase::Moved,
                };
                wheel_messages.write(wheel);
                if let Some(window_events) = window_messages.as_mut() {
                    window_events.write(WindowEvent::MouseWheel(wheel));
                }
                active = true;
            }
        }
    }

    // 3. Uncaptured movement only shows that the mouse is in use.
    active |= ingress::take_platform_mouse_hover();
    activity.0 = active;
}

#[cfg(test)]
mod tests;
