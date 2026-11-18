//! Tool-wear predictions and retained response history for block breaks.

use super::cells::{Cells, Held};
use super::queue::{CellPrediction, PendingRequest, RollbackAuthority, same_name};
use super::{Cell, InventoryPendingState, PlayerInventoryLedger};

/// The durability a mine-block request predicts for one hotbar slot.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct MiningPrediction {
    pub(super) slot: u8,
    pub(super) damage: i32,
    /// A client-authoritative destroy is committed backing named by this legacy ID.
    pub(super) legacy_request_id: Option<i32>,
    /// Identifies the selected item; accepted metadata stays in backing truth.
    source: Option<MiningSource>,
}

/// Stable item lineage for a HUD request, independent of mutable NBT and wire IDs.
#[derive(Debug, Clone, Eq, PartialEq)]
struct MiningSource {
    source_stack_id: i32,
    source_origin: Option<Cell>,
    network_id: i32,
    metadata: u32,
    block_runtime_id: i32,
}

impl MiningSource {
    /// Retains only the identity needed to reject responses for replacement items.
    fn new(held: &Held) -> Option<Self> {
        Some(Self {
            source_stack_id: held.source_stack_id?,
            source_origin: held.source_origin,
            network_id: held.stack.network_id,
            metadata: held.stack.metadata,
            block_runtime_id: held.stack.block_runtime_id,
        })
    }
}

impl MiningPrediction {
    /// Resolves this request's still-current item from authoritative backing, never history.
    pub(super) fn current_source<'a>(&self, cells: &'a Cells) -> Option<&'a Held> {
        let held = cells.get(Cell::Inventory(self.slot))?;
        (MiningSource::new(held).as_ref()? == self.source.as_ref()?).then_some(held)
    }
}

impl MiningPrediction {
    /// Matches retained mining lineage without restoring mutable item data from history.
    pub(super) fn retains_source_item(&self, held: &Held) -> bool {
        MiningSource::new(held)
            .as_ref()
            .is_some_and(|source| Some(source) == self.source.as_ref())
    }
}

/// Bounds outstanding mining predictions; the oldest is forgotten first.
const MAX_OUTSTANDING_MINING_REQUESTS: usize = 16;

impl PlayerInventoryLedger {
    /// Queues a mine-block prediction at its wire position: after every
    /// admitted request, ahead of unsent ones. `None` when the queue is full,
    /// so the break goes out without a request.
    pub(super) fn enqueue_mining(
        &mut self,
        slot: u8,
        damage: i32,
        legacy_request_id: Option<i32>,
    ) -> Option<i32> {
        if legacy_request_id.is_none() {
            self.ensure_queue_capacity().ok()?;
        }
        let request_id = self.next_request_id;
        self.next_request_id = request_id.checked_sub(2)?;
        let mining: Vec<usize> = self
            .queue
            .iter()
            .enumerate()
            .filter(|(_, request)| request.mining.is_some())
            .map(|(index, _)| index)
            .collect();
        if mining.len() >= MAX_OUTSTANDING_MINING_REQUESTS {
            self.queue.remove(mining[0]);
        }
        let position = self
            .queue
            .iter()
            .position(|request| request.state == InventoryPendingState::AwaitingTransport)
            .unwrap_or(self.queue.len());
        self.queue.insert(
            position,
            PendingRequest {
                request_id,
                actions: Vec::new(),
                filter_strings: Vec::new(),
                groups: Vec::new(),
                state: InventoryPendingState::AwaitingResponse,
                transport_deadline_millis: None,
                deadline_millis: None,
                timed_out: false,
                awaiting_refresh: Vec::new(),
                refresh_gates: false,
                session_generation: self.session_generation,
                storage_generation: None,
                personal_generation: None,
                storage_identity: None,
                trade_purchase: None,
                requires_distinct_stack_ids: false,
                registry_bound_merge: false,
                quick_move_route: None,
                predicted: Vec::new(),
                placements: Vec::new(),
                mining: Some(MiningPrediction {
                    slot,
                    damage,
                    legacy_request_id,
                    source: self
                        .confirmed
                        .get(Cell::Inventory(slot))
                        .and_then(MiningSource::new),
                }),
                world_drop: None,
                consumed_after: Vec::new(),
            },
        );
        Some(request_id)
    }

    /// Removes a cancelled mining request without changing backing cells.
    pub(super) fn remove_unanswered_mining(&mut self, request_id: i32) {
        self.queue
            .retain(|request| request.request_id != request_id || request.mining.is_none());
    }

    /// Reconciles wear whose movement carrier was revoked before a socket receipt.
    pub fn revoke_destroy_slot_change(
        &mut self,
        slot: u8,
        legacy_request_id: i32,
        definitely_unsent: bool,
    ) {
        if slot >= protocol::HOTBAR_SLOT_COUNT || !protocol::is_legacy_request_id(legacy_request_id)
        {
            return;
        }
        let cell = Cell::Inventory(slot);
        let index = self.queue.iter().position(|request| {
            request.mining.as_ref().is_some_and(|mining| {
                mining.slot == slot && mining.legacy_request_id == Some(legacy_request_id)
            })
        });
        if definitely_unsent && let Some(index) = index {
            let request = self.queue.remove(index).expect("destroy request observed");
            self.reject_request(&request);
            self.finish_request_retirement(request.request_id, false);
        } else if index.is_some()
            || self
                .confirmed
                .get(cell)
                .is_none_or(|held| held.stack.stack_network_id == legacy_request_id)
        {
            // Retired history or a pending socket write cannot prove a rollback safe.
            let before = self.confirmed.get(cell).cloned();
            let mut unverified = before.clone();
            if let Some(held) = &mut unverified {
                held.stack.stack_network_id = 0;
            }
            // Keep the backing owner's response snapshot under its unverified name.
            for prediction in self
                .queue
                .iter_mut()
                .filter(|request| request.locally_committed())
                .flat_map(|request| &mut request.predicted)
                .filter(|prediction| {
                    prediction.current
                        && prediction.cell == cell
                        && same_name(prediction.held.as_ref(), before.as_ref())
                })
            {
                prediction.held = unverified.clone();
            }
            self.confirmed.set(cell, unverified.clone());
            self.rebase_predictions(
                i32::MAX,
                cell,
                before.as_ref(),
                unverified,
                RollbackAuthority::Unknown,
            );
            self.require_rollback_recovery(cell);
        }
        self.refold();
    }

    /// Commits an admitted destroy's exact slot change, retaining its response and rollback.
    /// A replaced or already committed source cannot be applied a second time.
    pub fn settle_destroy_slot_change(
        &mut self,
        slot: u8,
        change: &protocol::PredictedSlotChange,
    ) -> bool {
        let cell = Cell::Inventory(slot);
        if slot >= protocol::HOTBAR_SLOT_COUNT
            || !protocol::is_legacy_request_id(change.legacy_request_id)
        {
            return false;
        }
        let Some(before) = self.confirmed.get(cell).cloned() else {
            return false;
        };
        let Ok(from) = protocol::VerifiedNetworkItemStack::try_new(
            before.stack.clone(),
            before.stack.nbt_digest,
        ) else {
            return false;
        };
        if from != change.from
            || (!change.to.is_empty()
                && (change.to.network_id() != from.network_id()
                    || change.to.metadata() != from.metadata()
                    || change.to.block_runtime_id() != from.block_runtime_id()
                    || change.to.stack_network_id() != change.legacy_request_id))
        {
            return false;
        }
        let damage = protocol::item_extra_damage_integer(change.to.extra_data()).unwrap_or(0);
        let Some(request_id) = self.enqueue_mining(slot, damage, Some(change.legacy_request_id))
        else {
            return false;
        };
        let after = (!change.to.is_empty()).then(|| {
            let mut held: Held = before.clone();
            held.stack.count = change.to.count();
            held.stack.stack_network_id = change.to.stack_network_id();
            held.stack.extra_data = change.to.extra_data().into();
            held.stack.nbt_digest = change.to.nbt_digest();
            held.overlay
                .get_or_insert_with(Default::default)
                .durability_correction = Some(damage);
            held
        });
        self.confirmed.set(cell, after.clone());
        self.rebase_predictions(
            i32::MAX,
            cell,
            Some(&before),
            after.clone(),
            RollbackAuthority::Known,
        );
        let request = self
            .queue
            .iter_mut()
            .find(|request| request.request_id == request_id)
            .expect("new mining request");
        request.predicted.push(CellPrediction {
            cell,
            before: Some(before),
            rollback: RollbackAuthority::Known,
            held: after,
            active: false,
            previous_owner: None,
            current: true,
            owns_empty_backing: false,
        });
        self.refold();
        true
    }
}
