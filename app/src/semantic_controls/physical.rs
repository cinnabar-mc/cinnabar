use bevy::{
    ecs::{entity::Entity, system::SystemParam},
    input::{
        gamepad::{Gamepad, GamepadAxis, GamepadButton},
        mouse::AccumulatedMouseMotion,
        touch::Touches,
    },
    prelude::{
        ButtonInput, KeyCode, MouseButton, Query, Res, ResMut, Resource, Single, Window, With,
    },
    window::{CursorOptions, PrimaryWindow},
};
use semantic_input::{
    ButtonEdges, ControllerFrame, DeviceFrame, KeyboardMouseFrame, MAX_CONTROLLERS,
    MAX_TOUCH_CONTACTS, ModifierChord, TouchContact,
};

use crate::camera::mouse_input_active;
mod ordered;
use {
    super::{SemanticInputRuntime, SemanticTouchTargets},
    client_state::SemanticInputSnapshot,
};

#[derive(Resource, Debug, Default)]
pub(crate) struct PendingDeviceFrame {
    frame: Option<DeviceFrame>,
    ignored_controllers: u64,
    ignored_touches: u64,
}

impl PendingDeviceFrame {
    /// Edits a collected desktop batch for input-routing tests, preserving held controls.
    #[cfg(test)]
    pub(crate) fn edit_desktop_events(
        &mut self,
        edit: impl FnOnce(&mut semantic_input::DesktopEvents),
    ) {
        edit(
            self.frame
                .as_mut()
                .and_then(|frame| frame.desktop_events.as_mut())
                .expect("collected desktop events"),
        );
    }

    /// Replaces the current platform sample before the common semantic router runs.
    pub(crate) fn replace(&mut self, frame: DeviceFrame) {
        self.frame = Some(frame);
    }
}

#[derive(Resource, Debug, Default)]
pub(crate) struct SemanticRouteState {
    routed: bool,
}

#[derive(SystemParam)]
pub(crate) struct SemanticPhysicalInputs<'w, 's> {
    window: Single<'w, 's, (Entity, &'static Window, &'static CursorOptions), With<PrimaryWindow>>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse_buttons: Res<'w, ButtonInput<MouseButton>>,
    mouse_motion: Res<'w, AccumulatedMouseMotion>,
    gamepads: Query<'w, 's, (Entity, &'static Gamepad)>,
    touches: Res<'w, Touches>,
    touch_targets: ResMut<'w, SemanticTouchTargets>,
    focus: Option<Res<'w, client_state::CursorFocus>>,
    driven: Option<Res<'w, client_state::DrivenInput>>,
    reserved: Option<Res<'w, crate::semantic_controls::ReservedControls>>,
    effective_mode: Option<Res<'w, client_state::EffectiveGameplayInputMode>>,
    platform_buttons: Option<Res<'w, crate::platform_devices::PlatformButtonModifiers>>,
}

pub(crate) fn collect_raw_input(
    inputs: SemanticPhysicalInputs,
    mut pending: ResMut<PendingDeviceFrame>,
    mut input_gated: bevy::prelude::Local<bool>,
    events: Option<Res<bevy::prelude::Messages<bevy::window::WindowEvent>>>,
    mut cursor: bevy::prelude::Local<bevy::ecs::message::MessageCursor<bevy::window::WindowEvent>>,
) {
    let mode = inputs.effective_mode.as_ref().map(|m| m.0);
    let ordered = ordered::collect(&inputs, events.as_deref(), &mut cursor);
    let mut translated = translate_device_frame(inputs);
    if let Some((events, lost)) = ordered {
        translated.frame.window_focus_lost |= lost;
        if mode != Some(semantic_input::InputMode::Touch) {
            translated.frame.desktop_events = Some(events);
        }
    }
    let gated = translated.frame.window_focus_lost;
    if gated && !*input_gated {
        bevy::log::info!("held inputs released: window focus or input availability lost");
    }
    *input_gated = gated;
    pending.ignored_controllers = pending
        .ignored_controllers
        .saturating_add(translated.ignored_controllers as u64);
    pending.ignored_touches = pending
        .ignored_touches
        .saturating_add(translated.ignored_touches as u64);
    pending.replace(translated.frame);
}

pub(crate) fn route_semantic_input(
    mut pending: ResMut<PendingDeviceFrame>,
    mut runtime: ResMut<SemanticInputRuntime>,
    mut route: ResMut<SemanticRouteState>,
    consent: Option<Res<client_state::ConsentInput>>,
    effective_mode: Option<Res<client_state::EffectiveGameplayInputMode>>,
) {
    if let Some(mode) = effective_mode {
        let preferred = Some(mode.0);
        if runtime.preferred_input_mode() != preferred {
            runtime.set_preferred_input_mode(preferred);
        }
    }
    if consent.is_some_and(|consent| consent.0) {
        pending.frame = Some(DeviceFrame {
            window_focus_lost: true,
            ..Default::default()
        });
    }
    route.routed = pending
        .frame
        .take()
        .is_some_and(|frame| runtime.route_device_frame(frame).is_ok());
}

pub(crate) fn finalize_semantic_input_after_ui_authority(
    mut runtime: ResMut<SemanticInputRuntime>,
    mut route: ResMut<SemanticRouteState>,
    mut published: ResMut<SemanticInputSnapshot>,
    emote_input: Option<Res<client_state::ScreenInputConsumed>>,
) {
    let routed = std::mem::take(&mut route.routed);
    if !routed {
        published.clear();
        return;
    }
    if emote_input.is_some_and(|consumed| consumed.0) {
        // Raw devices were sampled before the wheel could open and close. Retire
        // that owned sample through the router's normal UI release/quarantine.
        runtime.release_all(semantic_input::ReleaseReason::UiFocusTaken);
    }
    match runtime.finalize_routed_input() {
        Ok(snapshot) => published.replace(snapshot),
        Err(_) => published.clear(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InputSourceGates {
    keyboard_mouse: bool,
    controllers_and_touch: bool,
}

/// Whether gameplay admits input from `mode`'s devices: focus admits controllers and touch,
/// while keyboard and mouse also need cursor capture.
pub(crate) const fn admits_input_mode(
    mode: semantic_input::InputMode,
    window_focused: bool,
    cursor_captured: bool,
) -> bool {
    let gates = input_source_gates(window_focused, cursor_captured);
    match mode {
        semantic_input::InputMode::KeyboardMouse => gates.keyboard_mouse,
        semantic_input::InputMode::GamePad | semantic_input::InputMode::Touch => {
            gates.controllers_and_touch
        }
    }
}

const fn input_source_gates(window_focused: bool, cursor_captured: bool) -> InputSourceGates {
    InputSourceGates {
        keyboard_mouse: window_focused && cursor_captured,
        controllers_and_touch: window_focused,
    }
}

#[derive(Debug)]
struct BoundedSamples<T> {
    samples: Vec<T>,
    ignored: usize,
}

fn select_lowest_by_key<T>(
    samples: impl IntoIterator<Item = T>,
    limit: usize,
    key: impl Fn(&T) -> u64,
) -> BoundedSamples<T> {
    let mut selected = Vec::with_capacity(limit);
    let mut observed = 0_usize;
    for sample in samples {
        observed = observed.saturating_add(1);
        if limit == 0 {
            continue;
        }
        let insertion = selected.partition_point(|existing| key(existing) <= key(&sample));
        if selected.len() < limit {
            selected.insert(insertion, sample);
        } else if insertion < limit {
            selected.pop();
            selected.insert(insertion, sample);
        }
    }
    BoundedSamples {
        ignored: observed.saturating_sub(selected.len()),
        samples: selected,
    }
}

#[derive(Debug)]
struct TranslatedDeviceFrame {
    frame: DeviceFrame,
    ignored_controllers: usize,
    ignored_touches: usize,
}

fn translate_device_frame(inputs: SemanticPhysicalInputs) -> TranslatedDeviceFrame {
    let SemanticPhysicalInputs {
        window,
        keys,
        mouse_buttons,
        mouse_motion,
        gamepads,
        touches,
        mut touch_targets,
        focus,
        driven,
        reserved: _,
        effective_mode,
        platform_buttons: _,
    } = inputs;
    let mode = effective_mode.as_ref().map(|m| m.0);
    let (_, window, cursor) = window.into_inner();
    let focused = driven.is_some()
        || (window.focused && focus.as_ref().is_none_or(|focus| focus.available()));
    let captured = mouse_input_active(window, cursor, focus.as_deref(), driven.is_some());
    let gates = input_source_gates(focused, captured);
    if !gates.controllers_and_touch {
        touch_targets.release_all();
        return TranslatedDeviceFrame {
            frame: DeviceFrame {
                window_focus_lost: true,
                ..DeviceFrame::default()
            },
            ignored_controllers: 0,
            ignored_touches: 0,
        };
    }

    let keyboard_mouse = (gates.keyboard_mouse && mode != Some(semantic_input::InputMode::Touch))
        .then(|| {
            let mut keyboard_keys = keys
                .get_pressed()
                .filter_map(|key| keyboard_usage(*key))
                .collect::<Vec<_>>();
            keyboard_keys.sort_unstable();
            keyboard_keys.dedup();
            let mut buttons = mouse_buttons
                .get_pressed()
                .filter_map(|button| mouse_button_code(*button))
                .collect::<Vec<_>>();
            buttons.sort_unstable();
            buttons.dedup();
            let sampled_key = |key| keys.pressed(key) || keys.just_pressed(key);
            KeyboardMouseFrame {
                activity_sequence: 0,
                keys: keyboard_keys,
                mouse_buttons: buttons,
                key_edges: ButtonEdges {
                    pressed: keys
                        .get_just_pressed()
                        .filter_map(|key| keyboard_usage(*key))
                        .collect(),
                    released: keys
                        .get_just_released()
                        .filter_map(|key| keyboard_usage(*key))
                        .collect(),
                },
                mouse_edges: ButtonEdges {
                    pressed: mouse_buttons
                        .get_just_pressed()
                        .filter_map(|button| mouse_button_code(*button))
                        .collect(),
                    released: mouse_buttons
                        .get_just_released()
                        .filter_map(|button| mouse_button_code(*button))
                        .collect(),
                },
                mouse_motion: mouse_motion.delta.to_array(),
                modifiers: ModifierChord {
                    shift: sampled_key(KeyCode::ShiftLeft) || sampled_key(KeyCode::ShiftRight),
                    control: sampled_key(KeyCode::ControlLeft)
                        || sampled_key(KeyCode::ControlRight),
                    alt: sampled_key(KeyCode::AltLeft) || sampled_key(KeyCode::AltRight),
                    super_key: sampled_key(KeyCode::SuperLeft) || sampled_key(KeyCode::SuperRight),
                },
            }
        });
    let bounded_gamepads = select_lowest_by_key(gamepads.iter(), MAX_CONTROLLERS, |(entity, _)| {
        u64::from(entity.index().index())
    });
    let mut controllers = Vec::with_capacity(bounded_gamepads.samples.len());
    for (entity, gamepad) in bounded_gamepads.samples {
        controllers.push(ControllerFrame {
            device_id: entity.index().index(),
            activity_sequence: 0,
            axes: [
                gamepad.get(GamepadAxis::LeftStickX).unwrap_or(0.0),
                gamepad.get(GamepadAxis::LeftStickY).unwrap_or(0.0),
                gamepad.get(GamepadAxis::RightStickX).unwrap_or(0.0),
                gamepad.get(GamepadAxis::RightStickY).unwrap_or(0.0),
                gamepad.get(GamepadButton::LeftTrigger2).unwrap_or(0.0),
                gamepad.get(GamepadButton::RightTrigger2).unwrap_or(0.0),
                0.0,
                0.0,
            ],
            buttons: gamepad_button_codes(gamepad),
            button_edges: ButtonEdges {
                pressed: TRANSLATED_GAMEPAD_BUTTONS
                    .iter()
                    .filter_map(|(code, button)| gamepad.just_pressed(*button).then_some(*code))
                    .collect(),
                released: TRANSLATED_GAMEPAD_BUTTONS
                    .iter()
                    .filter_map(|(code, button)| gamepad.just_released(*button).then_some(*code))
                    .collect(),
            },
        });
    }
    let (touches, ignored_touches) = if mode == Some(semantic_input::InputMode::KeyboardMouse) {
        touch_targets.release_all();
        (Vec::new(), 0)
    } else {
        let width = window.width().max(1.0);
        let height = window.height().max(1.0);
        let bounded_touches =
            select_lowest_by_key(touches.iter(), MAX_TOUCH_CONTACTS, |touch| touch.id());
        touch_targets
            .retain_active_contacts(bounded_touches.samples.iter().map(|touch| touch.id()));
        let mut contacts = Vec::with_capacity(bounded_touches.samples.len());
        for touch in bounded_touches.samples {
            let contact = TouchContact {
                contact_id: touch.id(),
                activity_sequence: 0,
                position: [
                    (touch.position().x / width).clamp(0.0, 1.0),
                    (touch.position().y / height).clamp(0.0, 1.0),
                ],
                delta: touch_targets
                    .delta(touch.id())
                    .unwrap_or([touch.delta().x / width, touch.delta().y / height]),
                hit_id: touch_targets.target(touch.id()),
            };
            if contact.hit_id.is_some() {
                contacts.push(contact);
            }
        }
        contacts.extend(touch_targets.take_pulses());
        let bounded_contacts =
            select_lowest_by_key(contacts, MAX_TOUCH_CONTACTS, |contact| contact.contact_id);
        (
            bounded_contacts.samples,
            bounded_touches.ignored + bounded_contacts.ignored,
        )
    };
    TranslatedDeviceFrame {
        frame: DeviceFrame {
            keyboard_mouse,
            controllers,
            touches,
            ..DeviceFrame::default()
        },
        ignored_controllers: bounded_gamepads.ignored,
        ignored_touches,
    }
}

use client_shell::physical_input::{TRANSLATED_GAMEPAD_BUTTONS, keyboard_usage, mouse_button_code};

fn gamepad_button_codes(gamepad: &Gamepad) -> Vec<u8> {
    let mut buttons = TRANSLATED_GAMEPAD_BUTTONS
        .iter()
        .filter_map(|(code, button)| gamepad.pressed(*button).then_some(*code))
        .collect::<Vec<_>>();
    buttons.sort_unstable();
    buttons
}

#[cfg(test)]
mod tests;
