//! Drag-distribute over several cells and double-click gather/transfer gestures.

use protocol::{NetworkItemStack, WindowKind};

use super::cells::{Cell, Held};
use super::gesture::{
    InventoryTarget, StackRequestActionKind, Submission, counted_merge, counted_transfer,
};
use super::registry::{OccupiedStackRelation, entry_capacity};
use super::{InventoryGestureError, PlayerInventoryLedger};

mod live;
pub use live::{DragDistribution, MAX_DISTRIBUTION_CELLS};

/// How a drag splits the cursor stack.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DistributeMode {
    /// Primary drag: an even share per cell, the remainder stays held.
    Even,
    /// Secondary drag: one item per cell.
    One,
}

impl PlayerInventoryLedger {
    /// Pulls every compatible stack in the open screen into the cursor stack,
    /// partial stacks first, until the cursor is full.
    pub fn begin_gather(&mut self) -> Result<i32, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        self.check_surfaces([Cell::Cursor])?;
        let mut cursor = self
            .named(self.view().get(Cell::Cursor).cloned())?
            .ok_or(InventoryGestureError::EmptyGesture)?;
        let address = self.window_address();
        let capacity = self
            .negotiated_item_entry(cursor.stack.network_id)
            .and_then(entry_capacity)
            .map(u16::from)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let cells = self.gather_cells();
        let mut actions = Vec::new();
        let mut groups = Vec::new();
        let (mut distinct, mut registry_bound) = (false, false);
        for pass in 0..2 {
            for cell in &cells {
                if cursor.stack.count >= capacity {
                    break;
                }
                let Some(held) = self.view().get(*cell) else {
                    continue;
                };
                if self.awaiting_identity(held) {
                    continue;
                }
                let OccupiedStackRelation::Compatible { capacity: cap } =
                    self.occupied_stack_relation(held, &cursor)
                else {
                    continue;
                };
                // Partial stacks go first; full ones only top up what is left.
                if (held.stack.count >= cap) != (pass == 1) {
                    continue;
                }
                let amount = held.stack.count.min(cap.saturating_sub(cursor.stack.count));
                if amount == 0 {
                    continue;
                }
                let built = counted_merge(
                    StackRequestActionKind::Take,
                    *cell,
                    Cell::Cursor,
                    &held.stack,
                    &cursor.stack,
                    address,
                    amount,
                    cap,
                )?;
                cursor.stack.count += amount;
                distinct |= built.requires_distinct_stack_ids;
                registry_bound |= built.registry_bound_merge;
                actions.push(built.action);
                groups.push(built.group);
            }
        }
        if actions.is_empty() {
            return Err(InventoryGestureError::InvalidRequest);
        }
        self.submit(Submission {
            actions,
            groups,
            personal_generation,
            requires_distinct_stack_ids: distinct,
            registry_bound_merge: registry_bound,
        })
    }

    /// Moves every compatible player-inventory stack into the open inventory.
    ///
    /// The clicked item is the matching focus; the cursor only gates this
    /// gesture and is never moved. Existing destination stacks are filled before
    /// empty slots, and any remainder stays in the player inventory.
    pub fn begin_gather_to_storage(
        &mut self,
        target: InventoryTarget,
    ) -> Result<i32, InventoryGestureError> {
        self.begin_gather_to_storage_with_focus(target, None)
    }

    /// Moves compatible player stacks into the open inventory using the item that
    /// was under the first double-click, even if the first Shift-click already
    /// moved that stack before the second click arrived.
    pub fn begin_gather_to_storage_with_focus(
        &mut self,
        target: InventoryTarget,
        focus_stack: Option<NetworkItemStack>,
    ) -> Result<i32, InventoryGestureError> {
        if self.window_kind().is_none() {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let Cell::Inventory(slot) = target.cell() else {
            return Err(InventoryGestureError::InvalidRequest);
        };
        let personal_generation = self.gesture_preflight(true)?;
        self.check_target(Cell::Inventory(slot))?;
        let clicked = self.named(self.view().get(Cell::Inventory(slot)).cloned())?;
        let cursor = self.named(self.view().get(Cell::Cursor).cloned())?;
        if cursor.is_none() {
            return Err(InventoryGestureError::EmptyGesture);
        }
        let focus = focus_stack
            .as_ref()
            .and_then(Held::new)
            .or(clicked)
            .or_else(|| cursor.clone())
            .ok_or(InventoryGestureError::EmptyGesture)?;
        let destinations = self
            .window_kind()
            .map(|kind| {
                self.window_destinations(kind, focus.stack.network_id, focus.stack.metadata)
            })
            .unwrap_or_default();
        self.transfer_matching_stacks(
            focus,
            self.player_inventory_cells(),
            destinations,
            personal_generation,
        )
    }

    /// Moves every compatible stack from an open inventory into the player's inventory.
    ///
    /// The cursor remains untouched; it only authorizes the gesture. `focus_stack`
    /// preserves the clicked item when the first Shift-click emptied its source slot.
    pub fn begin_gather_to_player(
        &mut self,
        target: InventoryTarget,
    ) -> Result<i32, InventoryGestureError> {
        self.begin_gather_to_player_with_focus(target, None)
    }

    /// Moves matching open-inventory stacks into the player's inventory.
    pub fn begin_gather_to_player_with_focus(
        &mut self,
        target: InventoryTarget,
        focus_stack: Option<NetworkItemStack>,
    ) -> Result<i32, InventoryGestureError> {
        let target_cell = target.cell();
        if self.window_kind().is_none() && !matches!(target_cell, Cell::Craft(_)) {
            return Err(InventoryGestureError::InvalidRequest);
        }
        if !matches!(target_cell, Cell::Storage(_) | Cell::Craft(_)) {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let personal_generation = self.gesture_preflight(true)?;
        self.check_target(target_cell)?;
        let clicked = self.named(self.view().get(target_cell).cloned())?;
        let cursor = self.named(self.view().get(Cell::Cursor).cloned())?;
        if cursor.is_none() {
            return Err(InventoryGestureError::EmptyGesture);
        }
        let focus = focus_stack
            .as_ref()
            .and_then(Held::new)
            .or(clicked)
            .or_else(|| cursor.clone())
            .ok_or(InventoryGestureError::EmptyGesture)?;
        self.transfer_matching_stacks(
            focus,
            self.open_inventory_cells(),
            self.player_inventory_cells(),
            personal_generation,
        )
    }

    /// Builds direct Place actions for every compatible source stack in a direction.
    fn transfer_matching_stacks(
        &mut self,
        focus: Held,
        source_cells: Vec<Cell>,
        destinations: Vec<Cell>,
        personal_generation: Option<u64>,
    ) -> Result<i32, InventoryGestureError> {
        let item_capacity = self
            .negotiated_item_entry(focus.stack.network_id)
            .and_then(entry_capacity)
            .map(u16::from)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let mut sources: Vec<(Cell, Held, u16)> = source_cells
            .into_iter()
            .filter_map(|cell| {
                let held = self.view().get(cell).cloned()?;
                (!self.awaiting_identity(&held)
                    && matches!(
                        self.merge_item_relation(&held, &focus),
                        OccupiedStackRelation::Compatible { .. }
                    ))
                .then_some((cell, held.clone(), held.stack.count))
            })
            .collect();
        let mut actions = Vec::new();
        let mut groups = Vec::new();
        let (mut distinct, mut registry_bound) = (false, false);
        let address = self.window_address();

        // Merge into existing destination stacks before using empty slots.
        for destination in &destinations {
            let Some(mut into) = self.view().get(*destination).cloned() else {
                continue;
            };
            let OccupiedStackRelation::Compatible { capacity } =
                self.merge_item_relation(&focus, &into)
            else {
                continue;
            };
            let capacity = self
                .placement_capacity(*destination, &focus.stack)
                .map_or(capacity, |limit| limit.min(capacity));
            for (source, held, remaining) in &mut sources {
                if *remaining == 0 || into.stack.count >= capacity {
                    continue;
                }
                let amount = (*remaining).min(capacity - into.stack.count);
                let built = counted_merge(
                    StackRequestActionKind::Place,
                    *source,
                    *destination,
                    &held.stack,
                    &into.stack,
                    address,
                    amount,
                    capacity,
                )?;
                if actions.len() >= protocol::MAX_STACK_REQUEST_ACTIONS {
                    return Err(InventoryGestureError::InvalidRequest);
                }
                *remaining -= amount;
                into.stack.count += amount;
                distinct |= built.requires_distinct_stack_ids;
                registry_bound |= built.registry_bound_merge;
                actions.push(built.action);
                groups.push(built.group);
            }
        }

        // Put each remaining source into empty destinations, splitting when needed.
        for destination in &destinations {
            if self.view().get(*destination).is_some() {
                continue;
            }
            for (source, held, remaining) in &mut sources {
                if *remaining == 0 {
                    continue;
                }
                let limit = self
                    .placement_capacity(*destination, &held.stack)
                    .unwrap_or(item_capacity)
                    .min(item_capacity);
                let amount = (*remaining).min(limit);
                if amount == 0 {
                    continue;
                }
                let built = counted_transfer(
                    StackRequestActionKind::Place,
                    *source,
                    *destination,
                    &held.stack,
                    address,
                    Some(amount),
                )?;
                if actions.len() >= protocol::MAX_STACK_REQUEST_ACTIONS {
                    return Err(InventoryGestureError::InvalidRequest);
                }
                *remaining -= amount;
                distinct |= built.requires_distinct_stack_ids;
                actions.push(built.action);
                groups.push(built.group);
                break;
            }
        }
        if actions.is_empty() {
            return Err(InventoryGestureError::InvalidRequest);
        }
        self.submit(Submission {
            actions,
            groups,
            personal_generation,
            requires_distinct_stack_ids: distinct,
            registry_bound_merge: registry_bound,
        })
    }

    fn open_inventory_cells(&self) -> Vec<Cell> {
        let mut cells = (0..u8::MAX)
            .map(Cell::Storage)
            .take_while(|cell| self.confirmed.contains(*cell))
            .collect::<Vec<_>>();
        cells.extend(
            super::crafting_close::screen_input_cells(self.window_kind())
                .filter(|cell| self.confirmed.contains(*cell)),
        );
        cells
    }

    fn player_inventory_cells(&self) -> Vec<Cell> {
        (0..protocol::PLAYER_INVENTORY_SLOTS)
            .filter(|slot| {
                self.known[usize::from(*slot)] && self.confirmed.contains(Cell::Inventory(*slot))
            })
            .map(Cell::Inventory)
            .collect()
    }

    /// Cells a gather may pull from, in screen order; outputs are excluded.
    fn gather_cells(&self) -> Vec<Cell> {
        let window = self
            .storage
            .as_ref()
            .filter(|storage| storage.identity.is_some());
        let output_cell = window.and_then(|storage| match storage.kind {
            WindowKind::Furnace | WindowKind::BlastFurnace | WindowKind::Smoker => Some(2),
            _ => None,
        });
        let mut cells: Vec<Cell> = (0..u8::MAX)
            .map(Cell::Storage)
            .take_while(|cell| self.confirmed.contains(*cell))
            .filter(|cell| *cell != Cell::Storage(output_cell.unwrap_or(u8::MAX)))
            .collect();
        cells.extend(
            (0..protocol::PLAYER_INVENTORY_SLOTS)
                .filter(|slot| {
                    self.known[usize::from(*slot)]
                        && self.confirmed.contains(Cell::Inventory(*slot))
                })
                .map(Cell::Inventory),
        );
        cells
    }
}
