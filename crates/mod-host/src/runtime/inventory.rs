//! `inventory`: the vanilla inventory requests a player mod stages during a callback. They are
//! committed when the callback returns and handed to the client once.

use super::{Instance, State, cinnabar::extension::inventory, cinnabar::session::items};
use crate::InventoryRequest;
use anyhow::Result;
use mod_api::{MAX_INVENTORY_REQUESTS, MAX_ITEM_IDENTIFIER_BYTES};

/// This callback's staged requests and the last successful callback's.
#[derive(Default)]
pub(super) struct InventoryRequests {
    pending: Vec<InventoryRequest>,
    committed: Vec<InventoryRequest>,
}

impl InventoryRequests {
    /// A failed callback's requests never reach the client.
    pub(super) fn begin(&mut self) {
        self.pending.clear();
    }

    pub(super) fn commit(&mut self) {
        self.committed.append(&mut self.pending);
        self.committed.truncate(MAX_INVENTORY_REQUESTS);
    }

    pub(super) fn revoke(&mut self) {
        *self = Self::default();
    }
}

impl State {
    fn stage_inventory(&mut self, request: InventoryRequest) -> Result<Result<(), String>> {
        self.charge()?;
        if !self.grants.inventory {
            return Ok(Err("inventory permission denied".into()));
        }
        let requests = &mut self.inventory.pending;
        if requests.len() >= MAX_INVENTORY_REQUESTS {
            return Ok(Err("inventory request limit reached".into()));
        }
        requests.push(request);
        Ok(Ok(()))
    }
}

impl inventory::Host for State {
    fn swap_offhand(&mut self, slot: u8) -> Result<Result<(), String>> {
        if usize::from(slot) >= mod_api::PLAYER_STATE_INVENTORY_SLOTS {
            self.charge()?;
            return Ok(Err("invalid player slot".into()));
        }
        let Some(frame) = self.snapshot.as_ref().filter(|_| {
            self.controls.frame.focused
                && self.controls.frame.gameplay
                && !self.controls.frame.panel_open
        }) else {
            self.charge()?;
            return Ok(Err("offhand swap requires focused gameplay".into()));
        };
        self.stage_inventory(InventoryRequest::SwapOffhand {
            session: frame.session,
            slot,
        })
    }

    fn creative_take(&mut self, key: items::ItemKey, count: u8) -> Result<Result<(), String>> {
        if key.identifier.is_empty() || key.identifier.len() > MAX_ITEM_IDENTIFIER_BYTES {
            self.charge()?;
            return Ok(Err("invalid item identifier".into()));
        }
        if count == 0 {
            self.charge()?;
            return Ok(Err("count must be at least 1".into()));
        }
        self.stage_inventory(InventoryRequest::CreativeTake {
            identifier: key.identifier,
            aux: key.aux,
            count,
        })
    }

    fn fill_recipe(&mut self, network_id: u32) -> Result<Result<(), String>> {
        self.stage_inventory(InventoryRequest::FillRecipe { network_id })
    }
}

impl Instance {
    /// Consumes the committed requests once.
    pub(crate) fn take_inventory_requests(&mut self) -> Vec<InventoryRequest> {
        std::mem::take(&mut self.store.data_mut().inventory.committed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModGrants;

    fn state(inventory: bool) -> State {
        let grants = ModGrants {
            inventory,
            ..ModGrants::default()
        };
        State::new(grants, String::new(), super::super::Declared::default())
    }

    fn key(identifier: &str) -> items::ItemKey {
        items::ItemKey {
            identifier: identifier.into(),
            aux: 0,
        }
    }

    #[test]
    fn requests_need_the_grant_and_commit_only_with_their_callback() {
        let mut denied = state(false);
        assert!(
            inventory::Host::fill_recipe(&mut denied, 7)
                .unwrap()
                .is_err()
        );

        let mut granted = state(true);
        inventory::Host::creative_take(&mut granted, key("minecraft:stone"), 1)
            .unwrap()
            .unwrap();
        // A callback that fails never commits what it staged.
        granted.inventory.begin();
        assert!(granted.inventory.committed.is_empty());

        inventory::Host::fill_recipe(&mut granted, 7)
            .unwrap()
            .unwrap();
        granted.inventory.commit();
        assert_eq!(
            std::mem::take(&mut granted.inventory.committed),
            [InventoryRequest::FillRecipe { network_id: 7 }]
        );
    }

    #[test]
    fn a_callback_stages_a_bounded_number_of_valid_requests() {
        let mut granted = state(true);
        assert!(
            inventory::Host::creative_take(&mut granted, key(""), 1)
                .unwrap()
                .is_err()
        );
        assert!(
            inventory::Host::creative_take(&mut granted, key("minecraft:stone"), 0)
                .unwrap()
                .is_err()
        );
        for network_id in 0..u32::try_from(MAX_INVENTORY_REQUESTS).unwrap() {
            inventory::Host::fill_recipe(&mut granted, network_id)
                .unwrap()
                .unwrap();
        }
        assert!(
            inventory::Host::fill_recipe(&mut granted, 99)
                .unwrap()
                .is_err()
        );
        granted.inventory.commit();
        assert_eq!(granted.inventory.committed.len(), MAX_INVENTORY_REQUESTS);
    }
    /// Swap requests require focused gameplay, a valid source, and a successful callback.
    #[test]
    fn offhand_swaps_are_granted_bounded_and_transactional() {
        use crate::GameplaySnapshot;
        let mut state = state(true);
        assert!(
            inventory::Host::swap_offhand(&mut state, 0)
                .unwrap()
                .is_err()
        );
        state.snapshot = Some(GameplaySnapshot {
            session: 7,
            dimension: 0,
            eye: crate::GameplayVector3 {
                x: 0.,
                y: 0.,
                z: 0.,
            },
            yaw: 0.,
            pitch: 0.,
            attack_held: false,
            frame_seconds: 0.01,
            players: Vec::new(),
        });
        state.controls.frame.focused = true;
        state.controls.frame.gameplay = true;
        assert!(
            inventory::Host::swap_offhand(&mut state, 36)
                .unwrap()
                .is_err()
        );
        state.controls.frame.panel_open = true;
        assert!(
            inventory::Host::swap_offhand(&mut state, 0)
                .unwrap()
                .is_err()
        );
        state.controls.frame.panel_open = false;
        state.grants.inventory = false;
        assert!(
            inventory::Host::swap_offhand(&mut state, 0)
                .unwrap()
                .is_err()
        );
        state.grants.inventory = true;
        inventory::Host::swap_offhand(&mut state, 20)
            .unwrap()
            .unwrap();
        state.inventory.begin();
        state.inventory.commit();
        assert!(state.inventory.committed.is_empty());
        inventory::Host::swap_offhand(&mut state, 20)
            .unwrap()
            .unwrap();
        state.inventory.commit();
        assert_eq!(
            state.inventory.committed,
            [InventoryRequest::SwapOffhand {
                session: 7,
                slot: 20
            }]
        );
        state.inventory.revoke();
        assert!(state.inventory.committed.is_empty());
    }
}
