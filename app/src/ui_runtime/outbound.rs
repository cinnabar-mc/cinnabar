//! Shared outbound UI intent for native and browser session hosts.
use crate::runtime::network::resource_packs::PackApplication;
use bevy::prelude::DetectChangesMut;
use bevy::{
    prelude::{Res, ResMut, Time},
    time::Real,
};
use client_runtime::network::NetworkHandle;
use client_shell::ui_resources::UiRuntimeResource;
use client_ui::ui_runtime::interaction::{ChatFlushError, flush_chat_sends, flush_inventory_send};

/// Reconciles revoked destroy carriers before inventory gestures read their backing stacks.
pub(crate) fn reconcile_mining_inventory(
    mut movement: ResMut<client_interaction::movement::MovementTicker>,
    mut player_runtime: ResMut<client_state::PlayerRuntime>,
) {
    if let Some(revoked) = movement.take_revoked_mining() {
        revoked.settle_inventory(player_runtime.inventory.ledger_mut());
    }
}

/// Flushes committed inventory intent through the session FIFO, retaining backpressured packets.
pub(crate) fn flush_inventory_network(
    mut player_runtime: bevy::prelude::ResMut<client_state::PlayerRuntime>,
    time: Res<Time<Real>>,
    mut runtime: ResMut<UiRuntimeResource>,
    network: Res<NetworkHandle<PackApplication>>,
    mut movement: ResMut<client_interaction::movement::MovementTicker>,
) {
    if let Some(revoked) = movement.take_revoked_mining() {
        revoked.settle_inventory(player_runtime.inventory.ledger_mut());
    }
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    let flushed = flush_inventory_send(
        player_runtime.bypass_change_detection(),
        runtime.bypass_change_detection(),
        now_millis,
        |packet| network.send_inventory_packet(packet),
    );
    // An idle flush changes neither resource, so the UI frame may repeat.
    if !matches!(flushed, Ok(false)) {
        runtime.set_changed();
        player_runtime.set_changed();
    }
    if let Err(client_session::PacketSendError::Closed(_)) = flushed {
        runtime.inventory_transport_closed(&mut player_runtime);
    }
    while runtime.has_client_packet()
        && let Some(packet) = runtime.take_client_packet()
    {
        match network.send_inventory_packet(packet) {
            Ok(()) => {}
            Err(client_session::PacketSendError::Full(packet)) => {
                runtime.requeue_client_packet(packet);
                break;
            }
            Err(client_session::PacketSendError::Closed(_)) => break,
        }
    }
}

/// Sends queued chat and wake requests without crossing a replaced session.
pub(crate) fn flush_chat_network(
    player_runtime: bevy::prelude::Res<client_state::PlayerRuntime>,
    mut runtime: ResMut<UiRuntimeResource>,
    network: Res<NetworkHandle<PackApplication>>,
    mut fatal_error: ResMut<crate::runtime::shutdown::FatalError>,
) {
    if runtime.chat_autocomplete_requested() {
        runtime.service_pending_chat_autocomplete(&player_runtime);
    }
    if network.closed_command_has_pending_control() {
        return;
    }
    let runtime_id = runtime.local_runtime_id(&player_runtime);
    if runtime.wake_requested() {
        runtime.flush_wake_request(runtime_id, |packet| network.send_inventory_packet(packet));
    }
    // Without an unsent request the flush changes nothing.
    let unsent = runtime.pending_chat_sends().len() > runtime.in_flight_chat_sends();
    let chat: &mut client_ui::ui_runtime::UiRuntime = if unsent {
        &mut runtime
    } else {
        runtime.bypass_change_detection()
    };
    match flush_chat_sends(chat, 8, |session, sequence, action, packet| {
        match network.send_chat_packet(session, sequence, action, packet) {
            Err(client_session::PacketSendError::Closed(packet))
                if network.closed_command_has_pending_control() =>
            {
                Err(client_session::PacketSendError::Full(packet))
            }
            result => result,
        }
    }) {
        Ok(_) | Err(ChatFlushError::Transport(client_session::PacketSendError::Full(_))) => {}
        Err(ChatFlushError::Transport(client_session::PacketSendError::Closed(_))) => {
            crate::runtime::shutdown::record_fatal_error(
                &mut fatal_error.0,
                "chat send failed because the network command channel closed".to_owned(),
            );
        }
        Err(ChatFlushError::Packet(error)) => {
            crate::runtime::shutdown::record_fatal_error(
                &mut fatal_error.0,
                format!("queued chat packet became invalid: {error}"),
            );
        }
        Err(ChatFlushError::SessionChanged { expected, actual }) => {
            crate::runtime::shutdown::record_fatal_error(
                &mut fatal_error.0,
                format!(
                    "queued chat packet crossed a session boundary: expected {expected}, got {actual}"
                ),
            );
        }
    }
}
