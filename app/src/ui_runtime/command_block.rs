//! The command block editor's input, and the server request that opens it.

use crate::runtime::network::resource_packs::PackApplication;
use bevy::{
    ecs::message::{MessageCursor, Messages},
    input::{
        ButtonState,
        gamepad::Gamepad,
        keyboard::KeyboardInput,
        mouse::{AccumulatedMouseMotion, MouseButtonInput, MouseWheel},
    },
    prelude::{
        ButtonInput, KeyCode, Local, MessageReader, MouseButton, Query, Res, ResMut, Single, Time,
        Window, With,
    },
    time::Real,
    window::{CursorOptions, PrimaryWindow},
};
use client_runtime::network::NetworkHandle;
use client_shell::{
    menu::MenuRuntime,
    physical_input::{keyboard_key, set_cursor_capture},
    ui_resources::{UiPresentationResource, UiRuntimeResource},
};
use client_ui::ui_runtime::command_block::{CommandBlockEdit, conditional_bit, mode_of_block};
use client_ui::ui_runtime::forms::engine_input;

use crate::runtime::world::ClientWorld;
use crate::ui_runtime::forms::{engine_frame_input, read_pointer_edges};
use crate::ui_runtime::interaction::input_capture::restore_gameplay_input_after_chat;

/// Input sources only the editor's frames read.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct CommandBlockInputSources<'w, 's> {
    wheel: MessageReader<'w, 's, MouseWheel>,
    keyboard: MessageReader<'w, 's, KeyboardInput>,
    button_messages: Option<Res<'w, Messages<MouseButtonInput>>>,
    button_cursor: Local<'s, MessageCursor<MouseButtonInput>>,
    held: Local<'s, bool>,
    stick: Local<'s, [bool; 4]>,
    pads: Query<'w, 's, &'static Gamepad>,
    time: Option<Res<'w, Time<Real>>>,
    pointer_source: Option<Res<'w, crate::ui_runtime::touch_pointer::UiPointerSource>>,
}

impl CommandBlockInputSources<'_, '_> {
    /// Drops this frame's input and every edge in progress.
    fn clear(&mut self) {
        self.wheel.clear();
        self.keyboard.clear();
        *self.held = false;
        *self.stick = [false; 4];
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn drive_command_block(
    (player_runtime, window, mut focus, driven): (
        Res<client_state::PlayerRuntime>,
        Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
        Option<ResMut<client_state::CursorFocus>>,
        Option<Res<client_state::DrivenInput>>,
    ),
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut motion: ResMut<AccumulatedMouseMotion>,
    mut sources: CommandBlockInputSources,
    menu: Option<Res<MenuRuntime>>,
    presentation: Res<UiPresentationResource>,
    network: Option<Res<NetworkHandle<PackApplication>>>,
    mut client_world: ResMut<ClientWorld>,
    mut runtime: ResMut<UiRuntimeResource>,
    mut owned_last_frame: Local<bool>,
    collisions: Option<Res<client_interaction::movement::PhysicsCollisionRegistries>>,
) {
    let (window, mut cursor) = window.into_inner();
    let input_available = driven.is_some()
        || (window.focused && focus.as_ref().is_none_or(|focus| focus.available()));
    // A data-driven form owns the keys: a request made meanwhile opens once the form closes.
    if runtime.resource_pack_loading()
        || runtime.credits().owns_input()
        || runtime.data_driven_ui().owns_input()
    {
        sources.clear();
        *owned_last_frame = false;
        return;
    }
    if menu.as_ref().is_some_and(|menu| menu.is_visible()) {
        runtime.command_block_mut().close();
        sources.clear();
        *owned_last_frame = false;
        return;
    }
    if let Some(stream) = client_world.stream.as_mut()
        && let Some(position @ [x, y, z]) = stream.take_pending_command_block()
    {
        let key = world::BlockEntityKey::new(stream.current_dimension(), x, y, z);
        let actor = stream
            .collision_store()
            .block_entity(key)
            .and_then(|nbt| nbt.parse())
            .unwrap_or_default();
        let runtime_id = stream
            .collision_store()
            .sub_chunk(key.sub_chunk())
            .and_then(|chunk| chunk.runtime_id(0, (x & 15) as u8, (y & 15) as u8, (z & 15) as u8));
        let mode_id = stream.network_id_mode();
        let block = runtime_id.zip(collisions.as_deref());
        let name = block.and_then(|(id, registries)| registries.block_identifier(mode_id, id));
        let conditional = block
            .and_then(|(id, registries)| registries.block_canonical_state(mode_id, id))
            .and_then(conditional_bit);
        let translator = runtime.translator();
        let edit =
            CommandBlockEdit::load(position, mode_of_block(name), conditional, &actor, |key| {
                translator.lookup(key)
            });
        // The events of the frame that opened the editor belong to the game.
        sources.clear();
        runtime.command_block_mut().open(edit);
    }
    if !runtime.command_block().is_open() {
        sources.clear();
        if *owned_last_frame && !runtime.ui_focused(&player_runtime) && input_available {
            restore_gameplay_input_after_chat(&mut cursor, &mut keys, &mut mouse, &mut motion);
        }
        *owned_last_frame = false;
        return;
    }
    *owned_last_frame = true;
    set_cursor_capture(&mut cursor, false);
    let pointer = read_pointer_edges(
        &mouse,
        sources.button_messages.as_deref(),
        &mut sources.button_cursor,
        &mut sources.held,
    );
    let frame = presentation.command_block_frame().cloned();
    if input_available && let Some(frame) = frame {
        let held: Vec<_> = keys.get_pressed().map(|key| keyboard_key(*key)).collect();
        let key_presses: Vec<_> = sources
            .keyboard
            .read()
            .filter(|input| input.state == ButtonState::Pressed)
            .collect();
        let input = engine_input::EngineInput {
            keys: &held,
            ..engine_frame_input(
                window,
                sources
                    .pointer_source
                    .as_ref()
                    .is_some_and(|source| source.0),
                pointer,
                sources.wheel.read(),
                &key_presses,
                &mouse,
                menu.as_deref(),
                (sources.pads.iter(), &mut sources.stick),
                sources
                    .time
                    .as_ref()
                    .map_or(0.0, |time| time.elapsed_secs_f64()),
            )
        };
        runtime.command_block_mut().drive(&frame, input);
    } else {
        sources.wheel.clear();
        sources.keyboard.clear();
        *sources.stick = [false; 4];
        runtime.command_block_mut().cancel_pointer_input();
    }
    keys.reset_all();
    motion.delta = bevy::math::Vec2::ZERO;
    if !runtime.command_block().exit_requested() {
        return;
    }
    if let Some(focus) = focus.as_deref_mut() {
        focus.authorize_screen_return();
    }
    if !runtime.command_block_mut().finish(|packet| {
        network
            .as_deref()
            .ok_or(())?
            .send_inventory_packet(packet)
            .map_err(|_| ())
    }) {
        return;
    }
    if !runtime.ui_focused(&player_runtime) {
        restore_gameplay_input_after_chat(&mut cursor, &mut keys, &mut mouse, &mut motion);
        *owned_last_frame = false;
    }
}
