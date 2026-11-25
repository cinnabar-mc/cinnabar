//! Screen input drivers that only settle state on a frame without input.
use bevy::prelude::{Local, Res};
use client_shell::{input_activity::RawInput, menu::MenuRuntime, ui_resources::UiRuntimeResource};
use client_ui::ui_runtime::scene_stack::Scene;

use super::input_owner::top_scene;
use crate::runtime::world::ClientWorld;

/// Screen drivers whose work on a frame without raw input, with gameplay on top, the menu
/// hidden and no editor, emote or mob effects screen open, requested or playing, is only to
/// settle what the frame before left: the emote wheel, the mob effects screen, the sign and
/// command block editors and the chat pointer actions. Once a whole such frame has passed they
/// would do nothing, so the set runs only under [`screen_readers_due`].
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct IdleScreenReaders;

/// Whether [`IdleScreenReaders`] run: this frame or the one before carried raw input, a screen
/// above gameplay, a visible menu, consent-owned input, screen-consumed input, an open or
/// requested editor, an open emote wheel or mob effects screen, or a playing emote. The frame
/// after any of these still runs them, so state they keep from one frame to the next, such as
/// whether they owned the cursor, settles first.
#[allow(clippy::too_many_arguments)]
pub(crate) fn screen_readers_due(
    input: RawInput,
    player: Res<client_state::PlayerRuntime>,
    runtime: Res<UiRuntimeResource>,
    menu: Option<Res<MenuRuntime>>,
    consent: Option<Res<client_state::ConsentInput>>,
    consumed: Option<Res<client_state::ScreenInputConsumed>>,
    world: Option<Res<ClientWorld>>,
    mut busy_before: Local<bool>,
) -> bool {
    let busy = input.active()
        || menu.as_deref().is_some_and(MenuRuntime::is_visible)
        || consent.is_some_and(|consent| consent.0)
        || consumed.is_some_and(|consumed| consumed.0)
        || runtime.emotes().is_open()
        || runtime.emotes().playback().is_some()
        || runtime.sign_editor().is_open()
        || runtime.command_block().is_open()
        || runtime.command_block().requested()
        || runtime.mob_effect_screen().is_open()
        || world
            .as_deref()
            .and_then(|world| world.stream.as_ref())
            .is_some_and(|stream| stream.authority().has_pending_sign_edit())
        || top_scene(&runtime, &player, menu.as_deref()) != Scene::Gameplay;
    let due = busy || *busy_before;
    *busy_before = busy;
    due
}

#[cfg(test)]
mod tests;
