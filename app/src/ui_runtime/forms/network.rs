use super::{FormTransportError, flush_form_response};
use crate::runtime::network::resource_packs::PackApplication;
use bevy::prelude::{Res, ResMut};
use client_runtime::network::NetworkHandle;
use client_session::PacketSendError;
use client_shell::ui_resources::UiRuntimeResource;

pub(crate) fn flush_server_form_network(
    mut runtime: ResMut<UiRuntimeResource>,
    player: Res<client_state::PlayerRuntime>,
    network: Res<NetworkHandle<PackApplication>>,
    menu: Option<Res<client_shell::menu::MenuRuntime>>,
    world: Option<Res<crate::runtime::world::ClientWorld>>,
) {
    let session = runtime.session_id();
    // Terminal controls retire the session before another enqueue attempt.
    if network.closed_command_has_pending_control() {
        return;
    }
    // Each step borrows the runtime mutably only with something to send, so idle frames leave
    // it unchanged for the UI frame's revision.
    if runtime.has_boss_responses() {
        let _ = runtime.flush_boss_responses(|packet| {
            network
                .send_form_packet(session, packet)
                .map_err(|error| match error {
                    PacketSendError::Full(_) => FormTransportError::Full,
                    PacketSendError::Closed(_) => FormTransportError::Closed,
                })
        });
    }
    let runtime_id = runtime.local_runtime_id(&player);
    let credits_completed = if runtime.credits().has_pending() {
        runtime.credits_mut().flush(runtime_id, |packet| {
            network
                .send_form_packet(session, packet)
                .map_err(|error| match error {
                    PacketSendError::Full(_) => FormTransportError::Full,
                    PacketSendError::Closed(_) => FormTransportError::Closed,
                })
        })
    } else {
        Ok(false)
    };
    if matches!(credits_completed, Ok(true)) {
        bevy::log::info!(session, ?runtime_id, "credits completion queued for server");
    }
    if runtime.credits().owns_input() {
        return;
    }
    // Opening settings in a session asks the server for its settings form once.
    let in_settings = menu.as_ref().is_some_and(|menu| {
        menu.is_visible() && menu.screen() == launcher::menu::MenuScreen::Settings
    });
    let selected = in_settings
        && menu.as_ref().is_some_and(|menu| {
            menu.settings_section()
                == Some(launcher::menu::settings_sections::SettingSection::Server)
        });
    client_shell::ui_resources::update(&mut runtime, |runtime| {
        let store = runtime.server_forms_mut();
        store.select_settings_form(selected)
            | store.flush_settings_request(in_settings, || {
                network
                    .send_form_packet(session, protocol::server_settings_request_packet())
                    .is_ok()
            })
    });
    if !runtime.server_forms().has_responses() {
        return;
    }
    let _ = flush_form_response(
        &mut runtime,
        |actor| {
            world
                .as_ref()
                .and_then(|world| world.stream.as_ref())
                .is_some_and(|stream| {
                    stream.authority().actor_lifetime(actor.runtime_id) == Some(actor)
                })
        },
        |packet| {
            network
                .send_form_packet(session, packet)
                .map_err(|error| match error {
                    PacketSendError::Full(_) => FormTransportError::Full,
                    PacketSendError::Closed(_) => FormTransportError::Closed,
                })
        },
    );
}

#[cfg(test)]
mod tests;
