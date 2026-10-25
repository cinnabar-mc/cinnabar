//! Physical input for vanilla's mob effect screen: the Mob Effects binding opens and closes
//! it, and Escape, Back, the controller's cancel button and its close button dismiss it.
use bevy::{
    ecs::system::SystemParam,
    input::{
        ButtonState,
        gamepad::{Gamepad, GamepadButton},
        keyboard::KeyboardInput,
        mouse::AccumulatedMouseMotion,
    },
    prelude::*,
    time::Real,
    window::{CursorOptions, PrimaryWindow},
};
use client_shell::menu::{
    MenuRuntime,
    settings_options::control_bindings::{binding_key, gamepad_button},
};
use client_shell::physical_input::set_cursor_capture;
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
use launcher::menu::settings_options::BindingId;
use ui::UiPoint;

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
}

/// Runs after the emote wheel, which clears [`client_state::ScreenInputConsumed`] each frame,
/// and before chat, inventory and menu input see the frame.
pub(crate) fn drive_mob_effect_screen_input(mut input: MobEffectInput) {
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
        || input.runtime.chat_focused()
        || input.runtime.inventory_open()
        || input.runtime.server_forms().owns_input()
        || input.runtime.data_driven_ui().owns_input()
        || input.runtime.sign_editor().active().is_some()
        || input.runtime.local_sleeping()
        || input.runtime.emotes().is_open()
        || actor_unavailable;
    let keyboard: Vec<_> = input.keyboard.read().cloned().collect();
    if blocked || !input.window.0.focused {
        input.modifiers.reset_all();
        input.runtime.close_mob_effect_screen();
        return;
    }
    let now = u64::try_from(input.time.elapsed().as_millis()).unwrap_or(u64::MAX);
    let was_open = input.runtime.mob_effect_screen().is_open();
    let mut owned = was_open;
    chat_modifiers::capture_before_events(&mut input.modifiers, &input.keys, &keyboard);
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
                chat_modifiers::held_chord(&input.modifiers),
                client_shell::physical_input::keyboard_usage(event.key_code)
                    .map(semantic_input::PhysicalControl::KeyboardUsage),
            )
            .is_none()
        {
            input.runtime.toggle_mob_effect_screen(now);
            owned |= open || input.runtime.mob_effect_screen().is_open();
        } else if open && event.key_code == KeyCode::Escape {
            input.runtime.close_mob_effect_screen();
        }
    }
    if input.runtime.mob_effect_screen().is_open() {
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
            || input.mouse.just_pressed(MouseButton::Back)
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

#[cfg(test)]
mod tests;
