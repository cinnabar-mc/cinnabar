//! Orders frame-time presses and tick-bound interaction payloads in the session FIFO.
#[cfg(feature = "acceptance")]
use crate::runtime::movement_evidence::emit_movement_evidence;
use crate::{
    runtime::telemetry::send_player_auth_inputs, ui_runtime::outbound::flush_inventory_network,
};
use bevy::prelude::*;
use client_state::ClientFrameSet;

/// Admits attacks before their frame's movement, then drains inputs before item uses.
pub(crate) fn configure(app: &mut App) {
    app.add_systems(
        Update,
        crate::block_use::retain_block_use_pick
            .run_if(resource_exists::<client_interaction::movement::PhysicsCollisionRegistries>)
            .in_set(client_state::ClientInteractionSet::RetainPick)
            .in_set(client_presentation::local_player::LocalPlayerFrameSet::Interaction)
            .in_set(ClientFrameSet::Interaction)
            .before(ClientFrameSet::NetworkSend),
    );
    app.add_systems(
        Update,
        (
            flush_inventory_network,
            #[cfg(feature = "acceptance")]
            emit_movement_evidence,
            #[cfg(not(feature = "acceptance"))]
            crate::runtime::telemetry::discard_completed_movement_evidence,
            crate::melee::prepare_melee,
            crate::melee::produce_attacks,
            crate::item_use::prepare_item_release,
            #[cfg(feature = "tracy")]
            crate::tracy::plot_physics_to_send,
            send_player_auth_inputs,
            crate::item_use::produce_item_use,
            crate::pick_block::produce_pick_block,
        )
            .chain()
            .in_set(ClientFrameSet::NetworkSend),
    );
}
