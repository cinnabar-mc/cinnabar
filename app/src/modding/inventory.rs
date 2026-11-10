//! Player mods' committed inventory requests, made as the vanilla creative catalog and recipe
//! book make them; the server checks each one.

use bevy::prelude::*;
use client_shell::ui_resources::UiRuntimeResource;
use inventory::inventory_ledger::InventoryGestureError;
use mod_host::InventoryRequest;

use super::ModRuntime;
use crate::runtime::world::ClientWorld;

pub(super) fn apply_inventory_requests(
    extension: Option<ResMut<ModRuntime>>,
    mut player_runtime: ResMut<client_state::PlayerRuntime>,
    mut ui: ResMut<UiRuntimeResource>,
    world: Option<Res<ClientWorld>>,
) {
    let Some(mut extension) = extension else {
        return;
    };
    let actor_session = world
        .as_ref()
        .and_then(|world| world.stream.as_ref())
        .map(|stream| stream.authority().actor_session_id());
    for index in 0..extension.hosts.host_count() {
        for request in extension.hosts.host_mut(index).take_inventory_requests() {
            let result = apply_request(&request, actor_session, &mut player_runtime, &mut ui);
            if let Err(error) = result {
                debug!(?request, ?error, "mod inventory request refused");
            }
        }
    }
}

/// Resolves a committed request against the same actor-session namespace the guest observed.
fn apply_request(
    request: &InventoryRequest,
    actor_session: Option<u64>,
    player: &mut player_state::PlayerState,
    ui: &mut client_ui::ui_runtime::UiRuntime,
) -> Result<i32, InventoryGestureError> {
    match request {
        InventoryRequest::SwapOffhand { session, slot } => {
            if Some(*session) != actor_session {
                return Err(InventoryGestureError::AuthorityUnavailable);
            }
            ui.mod_swap_offhand(player, *slot)
        }
        InventoryRequest::CreativeTake {
            identifier,
            aux,
            count,
        } => ui.mod_creative_take(player, identifier, *aux, *count),
        InventoryRequest::FillRecipe { network_id } => ui.mod_fill_recipe(player, *network_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{ContainerIdentity, InventoryContentEvent, InventoryEvent, NetworkItemStack};
    use std::sync::Arc;

    /// Player inventory generation deliberately differs from the actor-session identity.
    fn player() -> player_state::PlayerState {
        let mut player = player_state::PlayerState::new(91);
        let ledger = player.inventory.ledger_mut();
        ledger.apply(&InventoryEvent::Authority(
            protocol::InventoryAuthority::Server,
        ));
        ledger.apply_registry(&protocol::ItemRegistryEvent {
            entries: Arc::from([protocol::ItemRegistryEntry {
                identifier: Arc::from(assets::facts::TOTEM_IDENTIFIER),
                network_id: 6,
                component_based: false,
                version: protocol::ItemRegistryVersion::None,
                components: None,
                component_digest: [0; 32],
                negotiated_max_stack_size: None,
                canonical_empty_component_data: true,
                item_tags: Arc::from([]),
            }]),
        });
        let mut slots =
            vec![NetworkItemStack::default(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)];
        slots[20] = NetworkItemStack {
            network_id: 6,
            count: 1,
            stack_network_id: 100,
            ..Default::default()
        };
        ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots: Arc::from(slots),
            storage_item: NetworkItemStack::default(),
        }));
        ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
            slots: Arc::from([NetworkItemStack::default()]),
            storage_item: NetworkItemStack::default(),
        }));
        player
    }

    #[test]
    fn offhand_requests_use_actor_sessions_and_refuse_stale_or_disconnected_requests() {
        use inventory::inventory_ledger::InventoryTarget;
        let request = InventoryRequest::SwapOffhand {
            session: 7,
            slot: 20,
        };
        let mut player = player();
        let mut ui = client_ui::ui_runtime::UiRuntime::new(1);
        for actor_session in [None, Some(8), Some(player.facts.session_id())] {
            assert_eq!(
                apply_request(&request, actor_session, &mut player, &mut ui),
                Err(InventoryGestureError::AuthorityUnavailable)
            );
            assert_eq!(player.inventory.ledger().pending_request_count(), 0);
        }
        apply_request(&request, Some(7), &mut player, &mut ui).unwrap();
        let ledger = player.inventory.ledger();
        assert!(ledger.displayed_stack(20).is_none());
        assert_eq!(
            ledger
                .target_stack(InventoryTarget::Offhand)
                .unwrap()
                .network_id,
            6
        );
        assert_eq!(ledger.pending_request_count(), 1);
    }
}
