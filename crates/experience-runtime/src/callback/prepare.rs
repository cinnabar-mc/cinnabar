//! Checking a callback request and building the callback's host value and export from it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use block_transform::overlay_face::MAX_CELL_OVERLAYS;

use super::{
    CallbackRes, Export, Slot, Snapshot, column, containers, decode, entities, items, overlays,
    redstone,
};
use crate::host::cinnabar::experience_server::types::{self as wit, CallbackInfo};
use crate::limits::{MAX_NETWORK_BLOCKS, MAX_NETWORK_DATA_BYTES, MAX_VALUE_DEPTH};
use crate::load::Catalog;
use crate::protocol::{self, BlockPos, Call, Cell, Network, Request};
use crate::value;

/// The callback's host value and export for `request`, an Experience whose blocks are `own`.
/// The anchor, whose chunk column bounds writes, is the call's position; a client message and an
/// epoch have their player's focus, if any, and without one no snapshot. Hex is decoded and player ids are checked here, so a request
/// that is not a callback, holds bad hex or a player id that is not canonical fails before
/// anything runs.
pub(super) fn prepare<'a>(
    own: &Arc<Catalog>,
    request: &'a Request,
) -> Result<(CallbackRes, Export<'a>), String> {
    let Request::Callback {
        info,
        actor,
        world_min_y,
        world_max_y,
        data_budget,
        snapshot,
        network,
        inventory,
        containers,
        call,
        ..
    } = request
    else {
        return Err("not a callback".to_owned());
    };
    player_id("actor", actor.as_deref())?;
    if let Some(context) = &info.context {
        entities::check_context(own, context)?;
    }
    let mut focused = None;
    let (anchor, export) = match call {
        Call::Entity { handle, event } => {
            if !snapshot.is_empty() {
                return Err("entity observation has a block snapshot".into());
            }
            let principal = match event {
                protocol::EntityEvent::Interact { player, .. }
                | protocol::EntityEvent::Attacked { player, .. } => Some(player.as_str()),
                protocol::EntityEvent::Mounted { rider }
                | protocol::EntityEvent::Dismounted { rider } => Some(rider.player.as_str()),
                protocol::EntityEvent::Steering { input } => Some(input.rider.player.as_str()),
                protocol::EntityEvent::Despawned {} => None,
            };
            player_id("entity actor", principal)?;
            if actor.as_deref() != principal {
                return Err("entity actor differs from observation".into());
            }
            (
                None,
                Export::Entity {
                    handle: (*handle).into(),
                    event: entities::event(event.clone())?,
                },
            )
        }
        Call::Tick { step } | Call::Timer { step, .. } => {
            if actor.is_some()
                || !snapshot.is_empty()
                || info.simulation.as_ref().is_none_or(|s| s.step != *step)
            {
                return Err(
                    "simulation callback has unauthorized snapshot or inconsistent time".into(),
                );
            }
            if let Call::Timer { handle, .. } = call
                && (info
                    .context
                    .as_ref()
                    .is_none_or(|c| c.current != handle.world)
                    || info
                        .simulation
                        .as_ref()
                        .is_none_or(|s| !s.schedules.contains(handle)))
            {
                return Err("scheduled callback has no active instance handle".into());
            }
            let export = match call {
                Call::Timer { handle, .. } => Export::Timer {
                    handle: handle.clone().into(),
                    step: (*step).into(),
                },
                _ => Export::Tick((*step).into()),
            };
            (None, export)
        }
        Call::Player { player, event } => {
            player_id("player", Some(player))?;
            if !snapshot.is_empty() || actor.as_deref().is_some_and(|actor| actor != player) {
                return Err("player observation has unauthorized snapshot or actor".into());
            }
            (
                None,
                Export::Player {
                    player,
                    event: event.clone().into(),
                },
            )
        }
        Call::Place { change } => (Some(change.pos), Export::Place(block_change(change)?)),
        Call::Break { change, .. } => (Some(change.pos), Export::Break(block_change(change)?)),
        Call::Interact {
            player,
            pos,
            face,
            hit,
        } => {
            player_id("player", Some(player))?;
            if let Some(hit) = hit
                && ![hit.x, hit.y, hit.z]
                    .iter()
                    .all(|axis| (0.0..=16.0).contains(axis))
            {
                return Err(format!("click position {hit:?} is not within the cell"));
            }
            let target = hit.and_then(|hit| {
                let anchor = snapshot.iter().find(|cell| cell.pos == *pos)?;
                overlays::target(own, &anchor.overlays, hit)
            });
            let export = Export::Interact {
                player,
                pos: (*pos).into(),
                face: (*face).into(),
                hit: hit.map(Into::into),
                target,
            };
            (Some(*pos), export)
        }
        Call::Neighbor { pos, neighbor } => (
            Some(*pos),
            Export::Neighbor {
                pos: (*pos).into(),
                neighbor: (*neighbor).into(),
            },
        ),
        Call::ClientMessage {
            player,
            channel,
            schema,
            payload,
            focus,
        } => {
            player_call(
                "a client message",
                player,
                actor.as_deref(),
                snapshot,
                *focus,
            )?;
            if value::depth(payload) > MAX_VALUE_DEPTH {
                let reason = format!("a client message nests values deeper than {MAX_VALUE_DEPTH}");
                return Err(reason);
            }
            let export = Export::ClientMessage {
                player,
                channel,
                schema: *schema,
                payload,
            };
            focused = *focus;
            (*focus, export)
        }
        Call::Epoch { player, focus } => {
            player_call("an epoch", player, actor.as_deref(), snapshot, *focus)?;
            focused = *focus;
            (*focus, Export::Epoch { player })
        }
        Call::ScheduledUpdate { pos } => {
            (Some(*pos), Export::ScheduledUpdate { pos: (*pos).into() })
        }
    };
    let cells = snapshot
        .iter()
        .map(|cell| {
            let data = decode(cell.data.as_deref())
                .map_err(|error| format!("data of cell {:?}: {error}", cell.pos))?;
            if !cell.overlays.is_empty() && !(cell.owned && own.overlay_hosts.contains(&cell.id)) {
                return Err(format!(
                    "cell {:?} has overlays but is not an overlay host of its own",
                    cell.pos
                ));
            }
            let slots = cell
                .overlays
                .iter()
                .map(|overlay| usize::from(overlay.slot));
            if !slots.is_sorted_by(|a, b| a < b)
                || cell
                    .overlays
                    .last()
                    .is_some_and(|overlay| usize::from(overlay.slot) >= MAX_CELL_OVERLAYS)
            {
                return Err(format!(
                    "the overlays of cell {:?} are not in distinct slots in slot order",
                    cell.pos
                ));
            }
            if cell.power > redstone::MAX_POWER
                || (cell.power > 0 && !(cell.owned && own.redstone_blocks.contains(&cell.id)))
            {
                return Err(format!(
                    "cell {:?} receives redstone power {}, more than {} or as no redstone block \
                     of its own",
                    cell.pos,
                    cell.power,
                    redstone::MAX_POWER
                ));
            }
            let slot = Slot {
                loaded: cell.loaded,
                id: cell.id.clone(),
                owned: cell.owned,
                data,
                states: cell.states.clone(),
                overlays: cell.overlays.clone(),
                power: cell.power,
            };
            Ok((cell.pos, slot))
        })
        .collect::<Result<HashMap<_, _>, String>>()?;
    let members = match network {
        Some(network) => members(network, &cells)?,
        None => HashSet::new(),
    };
    if let Some(inventory) = inventory {
        items::check_inventory(own, inventory, actor.as_deref())?;
    }
    let centres = anchor.into_iter().chain(members.iter().copied()).collect();
    let containers = containers::Reachable::new(own, centres, containers.as_ref())?;
    let res = CallbackRes {
        context: info.context.clone(),
        simulation: info.simulation.clone(),
        info: CallbackInfo {
            world_id: info.world_id.clone(),
            dimension_id: info.dimension_id.clone(),
            tick: info.tick,
            event_sequence: info.event_sequence,
        },
        actor: actor.clone(),
        player_store: actor.is_some() || matches!(call, Call::Player { .. }),
        focus: focused,
        harvest: matches!(call, Call::Break { harvest: true, .. }),
        interact: matches!(call, Call::Interact { .. }),
        own: Arc::clone(own),
        snapshot: Snapshot {
            cells,
            min_y: *world_min_y,
            max_y: *world_max_y,
            column: anchor.map(column),
            members,
        },
        network: network.clone(),
        inventory: inventory.clone(),
        containers,
        ops: Vec::new(),
        spawned_entities: 0,
        budget: *data_budget,
        added: 0,
        staged_data: 0,
        host_calls: 0,
        tells: 0,
        client_sends: 0,
        client_send_bytes: 0,
    };
    Ok((res, export))
}

/// The members of `network`, which the adapter builds from the snapshot's loaded own blocks
/// within the network bounds; anything else is malformed.
fn members(
    network: &Network,
    cells: &HashMap<BlockPos, Slot>,
) -> Result<HashSet<BlockPos>, String> {
    if network.blocks.len() > MAX_NETWORK_BLOCKS {
        return Err(format!(
            "the network has {} members; the limit is {MAX_NETWORK_BLOCKS}",
            network.blocks.len()
        ));
    }
    let mut data = 0;
    for pos in &network.blocks {
        let slot = cells
            .get(pos)
            .ok_or_else(|| format!("network member {pos:?} is not in the snapshot"))?;
        if !slot.loaded || !slot.owned {
            return Err(format!(
                "network member {pos:?} is not a loaded block of its own"
            ));
        }
        data += slot.data.as_ref().map_or(0, Vec::len);
    }
    if data > MAX_NETWORK_DATA_BYTES {
        return Err(format!(
            "the network holds {data} bytes of data; the limit is {MAX_NETWORK_DATA_BYTES}"
        ));
    }
    Ok(network.blocks.iter().copied().collect())
}

fn block_change(change: &protocol::Change) -> Result<wit::BlockChange, String> {
    player_id("change actor", change.actor.as_deref())?;
    let previous_data = decode(change.previous_data.as_deref())
        .map_err(|error| format!("previous data: {error}"))?;
    Ok(wit::BlockChange {
        pos: change.pos.into(),
        actor: change.actor.clone(),
        cause: change.cause.into(),
        before_id: change.before_id.clone(),
        after_id: change.after_id.clone(),
        previous_data,
    })
}

/// Refuses `id`, named `what`, when it is present but not a canonical player id.
pub(super) fn player_id(what: &str, id: Option<&str>) -> Result<(), String> {
    match id {
        Some(id) if !protocol::is_player_id(id) => Err(format!(
            "{what} is not a canonical lowercase hyphenated UUID"
        )),
        _ => Ok(()),
    }
}

/// Refuses `what`, a callback for `player` alone, unless `player` is canonical and the actor, and
/// the snapshot is empty without a `focus`.
fn player_call(
    what: &str,
    player: &str,
    actor: Option<&str>,
    snapshot: &[Cell],
    focus: Option<BlockPos>,
) -> Result<(), String> {
    player_id("player", Some(player))?;
    if actor != Some(player) {
        return Err(format!("{what}'s actor is not its player"));
    }
    if focus.is_none() && !snapshot.is_empty() {
        return Err(format!("{what} has a snapshot without a focus"));
    }
    Ok(())
}
