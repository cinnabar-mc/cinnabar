//! Inventory settlement for destroy transactions revoked with their movement carriers.

use super::MovementTicker;
use crate::mining::QueuedMiningInteraction;

/// Slot changes whose carriers were revoked, with whether delivery was ruled out.
#[derive(Debug, Clone, Default)]
pub struct MiningRevocations(Vec<(u8, i32, bool)>);

impl MiningRevocations {
    /// Retains the inventory identity before a revoked carrier loses its interaction.
    pub(super) fn record(&mut self, mining: QueuedMiningInteraction, definitely_unsent: bool) {
        if let Some(protocol::BlockItemInteraction::Destroy(request, Some(change))) =
            mining.interactions.block_interaction
        {
            self.0.push((
                request.selected_slot,
                change.legacy_request_id,
                definitely_unsent,
            ));
        }
    }

    /// Rolls back unsent wear and requests recovery for uncertain delivery.
    pub fn settle_inventory(self, ledger: &mut inventory::PlayerInventoryLedger) {
        for (slot, request, definitely_unsent) in self.0 {
            ledger.revoke_destroy_slot_change(slot, request, definitely_unsent);
        }
    }
}

impl MovementTicker {
    /// Takes revoked inventory work once, without touching the ledger on idle frames.
    pub fn take_revoked_mining(&mut self) -> Option<MiningRevocations> {
        (!self.revoked_mining.0.is_empty()).then(|| std::mem::take(&mut self.revoked_mining))
    }

    /// Revokes interactions while retaining inventory ownership for reconciliation.
    pub(super) fn invalidate_mining(&mut self) {
        let next = self.mining_epoch_publisher.borrow().wrapping_add(1);
        self.mining_epoch_publisher.send_replace(next);
        for queued in &mut self.outbox {
            if let Some(mining) = queued.mining.take() {
                self.revoked_mining.record(mining, true);
            }
        }
        for pending in &mut self.pending_sends {
            if let Some(mining) = pending.sample.mining.take() {
                self.revoked_mining.record(mining, false);
            }
        }
    }
}

#[cfg(test)]
mod tests;
