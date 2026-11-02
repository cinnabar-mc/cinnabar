//! Actor use shares the pre-physics pick and press admission with block use.
use client_world::game_mode_capabilities::SURVIVAL_ATTACK_REACH;
use gameplay::block_use::PickResult;
use simulation_clock::identity::SessionGeneration;
use {
    super::*,
    gameplay::block_use::{LocalUse, RepeatClock},
    gameplay::mining::{FrozenMiningSelection, creative_reach, survival_reach},
};

/// Current actor-use packet and the local effects committed on successful admission.
pub(crate) struct ActorUse {
    packet: protocol::Packet,
    local_use: LocalUse,
    tick: u64,
    due: std::time::Duration,
    clock: RepeatClock,
}

/// Resolves the actor/block competition with the same eligibility as ordinary block use.
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    context: &BlockUseContext,
    origin: &InteractionOriginSnapshot,
    selection: &FrozenMiningSelection,
    player: &client_state::PlayerRuntime,
    pick: FramePick,
    state: &gameplay::movement::UnsentSampleView,
    input_mode: PlayerInputMode,
    due: std::time::Duration,
    clock: RepeatClock,
) -> Option<ActorUse> {
    let stream = context.client_world.stream.as_ref()?;
    let ray = origin.outbound_ray()?;
    if !crate::interaction_authority::ray_is_current(
        ray,
        SessionGeneration::new(context.ui.session_id()),
        stream,
    ) || pick.session_generation != ray.session_generation()
        || pick.actor_session_id != ray.actor_session_id()
    {
        return None;
    }
    let reach = if clock.survival {
        survival_reach(input_mode)
    } else {
        creative_reach(input_mode)
    };
    let refreshed = pick.hits.ordinary?;
    let PickResult::Actor { identity, point } = refreshed else {
        return None;
    };
    let actor = super::pick::live_actor(stream.authority(), identity)?;
    let runtime_id = identity.runtime_id;
    let cap = player
        .facts
        .game_mode_capabilities()
        .map_or(SURVIVAL_ATTACK_REACH, |caps| caps.attack_reach);
    let hit = gameplay::melee::ActorHit {
        runtime_id,
        point,
        distance: f64::from(gameplay::melee::pick_distance(
            pick.origin.to_array(),
            point,
        )),
    };
    if !matches!(
        gameplay::melee::classify(Some(hit), None, cap),
        gameplay::melee::Crosshair::Actor(_)
    ) || matches!(refreshed.trimmed(state.position, reach), PickResult::Miss)
    {
        return None;
    }
    let packet = protocol::use_actor_packet(protocol::ActorUseRequest {
        actor_runtime_id: runtime_id,
        action: protocol::ActorUseAction::Interact,
        selected_slot: selection.slot,
        selected_item: selection.item.clone(),
        player_position: state.position,
        hit_position: point,
    })
    .ok()?;
    let local_use = LocalUse::for_actor(
        &actor.kind,
        context.ui.gameplay_hud().has_interact_text(),
        stream
            .authority()
            .offered_use(runtime_id, &use_equipment(player, &selection.item)),
    );
    Some(ActorUse {
        packet,
        local_use,
        tick: state.tick.saturating_add(1),
        due,
        clock,
    })
}

/// Reads the hands and armor that actor offerings test, keeping unobserved gear unknown.
fn use_equipment(
    player: &client_state::PlayerRuntime,
    main_hand: &protocol::VerifiedNetworkItemStack,
) -> client_world::UseEquipment {
    use inventory::{PlayerInventorySlot, inventory_ledger::InventoryTarget};
    let ledger = player.inventory.ledger();
    let gear = |target| match ledger.gear_slot_state(target) {
        Some(PlayerInventorySlot::Present(stack)) => Some(Some(stack)),
        Some(PlayerInventorySlot::Empty) => Some(None),
        Some(PlayerInventorySlot::Unknown) | None => None,
    };
    let offhand = gear(InventoryTarget::Offhand);
    let armor = (0..4).map(|slot| gear(InventoryTarget::Armor(slot)));
    client_world::UseEquipment {
        main_hand: client_world::HandStack::verified(main_hand),
        offhand: offhand
            .map(|stack| stack.map_or_else(Default::default, client_world::HandStack::of)),
        enchantments: client_world::equipment_enchantments(
            std::iter::once(Some(main_hand.extra_data()))
                .chain(std::iter::once(offhand).chain(armor).map(gear_extra_data)),
        ),
    }
}

/// An observed gear slot's user data, empty for an empty slot.
fn gear_extra_data(slot: Option<Option<&protocol::NetworkItemStack>>) -> Option<&[u8]> {
    slot.map(|stack| stack.map_or(&[][..], |stack| &stack.extra_data[..]))
}

/// Commits the current actor use only after its packet enters the session FIFO.
pub(crate) fn advance(
    context: &BlockUseContext,
    runtime: &mut BlockUseRuntime,
    attempt: &mut ActorUse,
) -> bool {
    match context
        .network
        .send_inventory_packets(vec![attempt.packet.clone()])
    {
        Ok(()) => {
            runtime.admit(
                ItemUseTrigger::PlayerInput,
                attempt.due,
                attempt.tick,
                attempt.local_use,
                attempt.clock,
                true,
            );
            true
        }
        Err(client_session::BatchSendError::Full) => false,
        Err(client_session::BatchSendError::Closed) => {
            runtime.clear_press();
            true
        }
    }
}

/// Handles an actor press through the shared immediate admission boundary.
#[allow(clippy::too_many_arguments)]
pub(super) fn produce(
    context: &BlockUseContext,
    runtime: &mut BlockUseRuntime,
    selection: &FrozenMiningSelection,
    player: &client_state::PlayerRuntime,
    pick: FramePick,
    state: &gameplay::movement::UnsentSampleView,
    input_mode: PlayerInputMode,
    trigger: ItemUseTrigger,
    due: std::time::Duration,
    clock: RepeatClock,
) -> bool {
    if trigger == ItemUseTrigger::SimulationTick {
        return false;
    }
    let Some(mut attempt) = prepare(
        context,
        &context.origin,
        selection,
        player,
        pick,
        state,
        input_mode,
        due,
        clock,
    ) else {
        return false;
    };
    advance(context, runtime, &mut attempt);
    true
}
