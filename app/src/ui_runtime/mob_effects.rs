//! Physical input for vanilla's mob effect screen: the Mob Effects binding opens and closes
//! it, and Escape, Menu Cancel, the controller's cancel button and its close button dismiss it.
use bevy::{
    ecs::message::{MessageCursor, Messages},
    ecs::system::SystemParam,
    input::{
        ButtonState,
        gamepad::{Gamepad, GamepadButton},
        keyboard::KeyboardInput,
        mouse::AccumulatedMouseMotion,
    },
    prelude::*,
    time::Real,
    window::{CursorOptions, PrimaryWindow, WindowEvent},
};
use client_shell::menu::{
    MenuRuntime,
    settings_options::control_bindings::{
        binding_key, binding_mouse_button, gamepad_button, menu_cancel_key, menu_cancel_mouse,
    },
};
use client_shell::physical_input::set_cursor_capture;
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
use client_ui::ui_runtime::scene_stack::Scene;
use launcher::menu::settings_options::BindingId;
use ui::UiPoint;

use super::interaction::input_capture::ordered_pointer_presses;
use super::interaction::{chat_modifiers, command_macros};
use crate::runtime::world::ClientWorld;
use client_state::PlayerRuntime;

#[derive(SystemParam)]
pub(crate) struct MobEffectInput<'w, 's> {
    time: Res<'w, Time<Real>>,
    window: Single<'w, 's, (&'static Window, &'static mut CursorOptions), With<PrimaryWindow>>,
    keys: ResMut<'w, ButtonInput<KeyCode>>,
    mouse: ResMut<'w, ButtonInput<MouseButton>>,
    motion: ResMut<'w, AccumulatedMouseMotion>,
    keyboard: MessageReader<'w, 's, KeyboardInput>,
    window_events: Option<Res<'w, Messages<WindowEvent>>>,
    window_cursor: Local<'s, MessageCursor<WindowEvent>>,
    pads: Query<'w, 's, &'static Gamepad>,
    menu: Option<Res<'w, MenuRuntime>>,
    consent: Option<Res<'w, client_state::ConsentInput>>,
    world: Option<Res<'w, ClientWorld>>,
    player: Res<'w, PlayerRuntime>,
    runtime: ResMut<'w, UiRuntimeResource>,
    presentation: Option<Res<'w, UiPresentationResource>>,
    modifiers: Local<'s, ButtonInput<KeyCode>>,
    focus: Option<ResMut<'w, client_state::CursorFocus>>,
    consumed: ResMut<'w, client_state::ScreenInputConsumed>,
    driven: Option<Res<'w, client_state::DrivenInput>>,
}

/// Runs after the emote wheel, which clears [`client_state::ScreenInputConsumed`] each frame,
/// and before chat, inventory and menu input see the frame. Its gates follow the wheel's in the
/// same order: another owner closes the screen; unavailable desktop input, then a frame the
/// wheel consumed, is drained without acting on it; then macros take precedence over the
/// binding.
pub(crate) fn drive_mob_effect_screen_input(mut input: MobEffectInput) {
    let pointer_presses = if let Some(events) = input.window_events.as_deref() {
        ordered_pointer_presses(input.window_cursor.read(events))
    } else {
        Vec::new()
    };
    let runtime_id = input.runtime.local_runtime_id(&input.player);
    let stream = input
        .world
        .as_deref()
        .and_then(|world| world.stream.as_ref());
    let actor_unavailable = runtime_id.is_none()
        || (input.world.is_some() && stream.is_none())
        || runtime_id
            .and_then(|id| stream.and_then(|stream| stream.authority().actor(id)))
            .is_some_and(|actor| actor.status.dead);
    let blocked = input.consent.as_ref().is_some_and(|consent| consent.0)
        || input.menu.as_ref().is_some_and(|menu| menu.is_visible())
        || input
            .runtime
            .screen_open_besides(&input.player, Some(Scene::MobEffects))
        || input.presentation.as_deref().is_some_and(|presentation| {
            presentation.mod_ui().mod_panel_open() || presentation.mod_ui().experience_modal_open()
        })
        || actor_unavailable;
    let keyboard: Vec<_> = input.keyboard.read().cloned().collect();
    if blocked {
        input.modifiers.reset_all();
        input.runtime.close_mob_effect_screen();
        return;
    }
    // Focus lost and regained in one batch leaves the window focused but the batch rejected.
    if input.driven.is_none()
        && (!input.window.0.focused || input.focus.as_ref().is_some_and(|focus| !focus.available()))
    {
        input.modifiers.reset_all();
        input.keys.reset_all();
        input.mouse.reset_all();
        input.motion.delta = Vec2::ZERO;
        input.consumed.0 |= input.runtime.mob_effect_screen().is_open();
        set_cursor_capture(&mut input.window.1, false);
        return;
    }
    if input.consumed.0 {
        input.modifiers.reset_all();
        return;
    }
    let now = u64::try_from(input.time.elapsed().as_millis()).unwrap_or(u64::MAX);
    let was_open = input.runtime.mob_effect_screen().is_open();
    let mut owned = was_open;
    chat_modifiers::capture_before_events(&mut input.modifiers, &input.keys, &keyboard);
    let mouse_toggled = mouse_binding_pressed(&input, &pointer_presses, &keyboard);
    if mouse_toggled {
        let open = input.runtime.mob_effect_screen().is_open();
        input.runtime.toggle_mob_effect_screen(now);
        owned |= open || input.runtime.mob_effect_screen().is_open();
    }
    for event in &keyboard {
        chat_modifiers::track(&mut input.modifiers, event);
        if event.state != ButtonState::Pressed || event.repeat {
            continue;
        }
        let open = input.runtime.mob_effect_screen().is_open();
        if binding_key(input.menu.as_deref(), BindingId::MobEffects, event.key_code)
            && command_macros::command(
                input.menu.as_deref(),
                &input.runtime,
                &input.player,
                chat_modifiers::held_chord(&input.modifiers),
                client_shell::physical_input::keyboard_usage(event.key_code)
                    .map(semantic_input::PhysicalControl::KeyboardUsage),
            )
            .is_none()
        {
            input.runtime.toggle_mob_effect_screen(now);
            owned |= open || input.runtime.mob_effect_screen().is_open();
        } else if open
            && (event.key_code == KeyCode::Escape
                || menu_cancel_key(input.menu.as_deref(), event.key_code))
        {
            input.runtime.close_mob_effect_screen();
        }
    }
    // A mouse press that toggled the screen this frame does not also dismiss it.
    if !mouse_toggled && input.runtime.mob_effect_screen().is_open() {
        let pointer = input
            .window
            .0
            .cursor_position()
            .and_then(|position| UiPoint::new(position.x, position.y).ok());
        let close_clicked = input.mouse.just_pressed(MouseButton::Left)
            && pointer.is_some_and(|point| {
                input
                    .presentation
                    .as_deref()
                    .is_some_and(|presentation| presentation.mob_effect_close_hit(point))
            });
        let cancel = input.menu.as_deref().map_or(GamepadButton::East, |menu| {
            gamepad_button(&menu.settings_snapshot().0, GamepadButton::East)
        });
        if close_clicked
            || menu_cancel_mouse(input.menu.as_deref(), &input.mouse).is_some()
            || input.pads.iter().any(|pad| pad.just_pressed(cancel))
        {
            input.runtime.close_mob_effect_screen();
        }
    }
    let open = input.runtime.mob_effect_screen().is_open();
    if was_open
        && !open
        && let Some(focus) = input.focus.as_deref_mut()
    {
        focus.authorize_screen_return();
    }
    if owned {
        input.consumed.0 = true;
        input.modifiers.reset_all();
        input.keys.reset_all();
        input.mouse.reset_all();
        input.motion.delta = Vec2::ZERO;
        set_cursor_capture(&mut input.window.1, !open);
    }
}

/// Whether a newly pressed mouse button is the Mob Effects binding and no command macro owns
/// it, judged with the modifiers held at that press's place among the frame's keys.
fn mouse_binding_pressed(
    input: &MobEffectInput,
    pointer_presses: &[(usize, MouseButton)],
    keyboard: &[KeyboardInput],
) -> bool {
    let menu = input.menu.as_deref();
    input.mouse.get_just_pressed().any(|button| {
        if !binding_mouse_button(menu, BindingId::MobEffects, *button) {
            return false;
        }
        let macro_matches = |held: &ButtonInput<KeyCode>| {
            command_macros::command(
                menu,
                &input.runtime,
                &input.player,
                chat_modifiers::held_chord(held),
                client_shell::physical_input::mouse_button_code(*button)
                    .map(semantic_input::PhysicalControl::MouseButton),
            )
            .is_some()
        };
        let macro_owns_press = if pointer_presses.is_empty() {
            macro_matches(&input.keys)
        } else {
            pointer_presses
                .iter()
                .filter(|(_, candidate)| candidate == button)
                .any(|(before, _)| {
                    let mut held = input.modifiers.clone();
                    for event in keyboard.iter().take(*before) {
                        chat_modifiers::track(&mut held, event);
                    }
                    macro_matches(&held)
                })
        };
        !macro_owns_press
    })
}

#[cfg(test)]
mod tests;
