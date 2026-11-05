//! Resolves explicit mod block positions without changing ordinary crosshair mining.

use super::MeleeContext;
use client_interaction::{mining::hand_interaction_selection, movement::MovementTicker};
use client_state::PlayerRuntime;
use protocol::BlockUseRequest;
use sim::PaletteWorld;

/// Resolves loaded cells and current permissions, attaching the complete admitted batch once.
pub(super) fn apply(
    player: &PlayerRuntime,
    context: &mut MeleeContext,
    movement: &mut MovementTicker,
    active: bool,
) {
    let Some(output) = context.mod_interaction.as_mut() else {
        return;
    };
    let Some(stream) = context.client_world.stream.as_mut() else {
        output.block_breaks.clear();
        return;
    };
    let caps = player.facts.game_mode_capabilities();
    if !active
        || context.focus.ui_focused
        || output.block_break_scope
            != Some((
                stream.authority().actor_session_id(),
                stream.current_dimension(),
            ))
        || !caps.is_some_and(|caps| caps.can_mine)
    {
        output.block_breaks.clear();
        return;
    }
    if output.block_breaks.is_empty() {
        return;
    }
    let Some(selection) = hand_interaction_selection(player) else {
        return;
    };
    let caps = caps.expect("checked mining capabilities");
    let mode = stream.network_id_mode();
    let world = PaletteWorld::new(
        stream.collision_store(),
        context.collisions.registry(mode),
        stream.current_dimension(),
        Default::default(),
    );
    let eye = context
        .origin
        .outbound_ray()
        .map(|ray| ray.origin().to_array());
    let Some(eye) = eye else {
        output.block_breaks.clear();
        return;
    };
    let requests: Vec<_> = output
        .block_breaks
        .iter()
        .filter_map(|&position| {
            let distance = position
                .into_iter()
                .zip(eye)
                .map(|(v, e)| (f64::from(v) + 0.5 - f64::from(e)).powi(2))
                .sum::<f64>();
            if distance > f64::from(mod_api::MAX_BLOCK_BREAK_DISTANCE).powi(2) {
                return None;
            }
            let id = world.primary_runtime_id(position).ok()?;
            if id == stream.air_block_id() {
                return None;
            }
            let held = stream.authority().held_edit_item(
                &selection.item,
                protocol::ItemBlockList::CanDestroy,
                id,
            );
            if !caps.permits_edit(held) {
                return None;
            }
            Some(BlockUseRequest {
                hand: protocol::ItemUseHand::Mainhand,
                block_position: position,
                face: 0,
                selected_slot: selection.slot,
                selected_item: selection.item.clone(),
                player_position: [0.; 3],
                relative_hit: [0.5; 3],
                block_runtime_id: u64::from(stream.block_network_id(id)?),
            })
        })
        .collect();
    let server_authoritative = player
        .facts
        .server_authoritative_block_breaking()
        .unwrap_or(true);
    if requests.is_empty()
        || movement.attach_requested_breaks(
            &requests,
            server_authoritative,
            caps.instant_break.then(|| stream.local_player_runtime_id()),
        )
    {
        for request in requests {
            stream.predict_block(request.block_position, 0, stream.air_block_id());
        }
        output.block_breaks.clear();
    }
}
