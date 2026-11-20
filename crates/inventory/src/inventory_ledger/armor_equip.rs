//! Right-click armor equips shown locally before the server restates the swap.

use protocol::{NetworkItemStack, VerifiedNetworkItemStack};

use super::cells::{ARMOR_CELLS, Cell, Held};
use super::queue::RollbackAuthority;
use super::{PLAYER_INVENTORY_SLOT_COUNT, PlayerInventoryLedger};

/// The armor slot and worn stack one right-click equip would swap with.
#[derive(Debug, Clone, PartialEq)]
pub struct ArmorEquipPreview {
    /// The destination armor container slot: `0` head through `3` feet or `4` body.
    pub armor_slot: u8,
    /// The armor worn in `armor_slot`, or an empty stack.
    pub worn: VerifiedNetworkItemStack,
}

impl PlayerInventoryLedger {
    /// The armor container slot `item` equips into, or `None` when it is not armor.
    ///
    /// Independent of whether the current contents allow showing the swap, so a
    /// caller can suppress armor repeats even when it cannot predict them.
    pub fn armor_equip_slot(&self, item: &VerifiedNetworkItemStack) -> Option<u8> {
        let slot = self.item_facts.armour_row(item.network_id())?;
        (usize::from(slot) < ARMOR_CELLS).then_some(slot)
    }

    /// Resolves the destination armor cell and worn stack for equipping `item`
    /// from hotbar `slot`, or `None` when the swap cannot be shown immediately.
    ///
    /// Refused while either cell has an unanswered prediction, before the armor
    /// container reports its contents, and for every removal or placement the
    /// server would refuse: a slot lock, Curse of Binding outside creative, a
    /// foreign armor row, or a stack larger than one item.
    pub fn armor_equip_preview(
        &self,
        slot: u8,
        item: &VerifiedNetworkItemStack,
    ) -> Option<ArmorEquipPreview> {
        let armor_slot = self.armor_equip_slot(item)?;
        let armor_index = usize::from(armor_slot);
        if usize::from(slot) >= PLAYER_INVENTORY_SLOT_COUNT
            || !self.armor_known.get(armor_index).copied()?
        {
            return None;
        }
        let source = Cell::Inventory(slot);
        let armor = Cell::Armor(armor_slot);
        if self.cell_pending(source) || self.cell_pending(armor) {
            return None;
        }
        let held = self.confirmed.get(source)?;
        if verified(Some(held))? != *item {
            return None;
        }
        let worn = self.confirmed.get(armor);
        self.validate_armor_equip(source, armor, held, worn).ok()?;
        Some(ArmorEquipPreview {
            armor_slot,
            worn: verified(worn)?,
        })
    }

    /// Applies a sent armor equip locally, retaining the source revision and
    /// both stacks the transaction named.
    ///
    /// `false` when any input no longer matches the retained backing, so the
    /// next authoritative armor update decides the shown value instead.
    pub fn settle_armor_equip(
        &mut self,
        revision: u64,
        change: &protocol::ArmorEquipChange,
    ) -> bool {
        let source = Cell::Inventory(change.slot);
        let Some(armor) = armor_cell(change.armor_slot) else {
            return false;
        };
        if self.authoritative_slot_revision(change.slot) != Some(revision)
            || !self
                .armor_known
                .get(usize::from(change.armor_slot))
                .copied()
                .unwrap_or(false)
            || !protocol::is_legacy_request_id(change.legacy_request_id)
            || self.cell_pending(source)
            || self.cell_pending(armor)
        {
            return false;
        }
        let Some(held) = self.confirmed.get(source).cloned() else {
            return false;
        };
        let worn = self.confirmed.get(armor).cloned();
        if verified(Some(&held)).as_ref() != Some(&change.from)
            || verified(worn.as_ref()).as_ref() != Some(&change.to)
            || self
                .validate_armor_equip(source, armor, &held, worn.as_ref())
                .is_err()
        {
            return false;
        }
        self.confirmed.set(source, worn.clone());
        self.confirmed.set(armor, Some(held.clone()));
        let equipped = self.confirmed.get(armor).cloned();
        self.rebase_predictions(
            i32::MAX,
            source,
            Some(&held),
            worn.clone(),
            RollbackAuthority::Known,
        );
        self.rebase_predictions(
            i32::MAX,
            armor,
            worn.as_ref(),
            equipped,
            RollbackAuthority::Known,
        );
        self.refold();
        true
    }
}

/// The armor container cell for a wire slot, `None` when no such cell is retained.
fn armor_cell(slot: u8) -> Option<Cell> {
    (usize::from(slot) < ARMOR_CELLS).then_some(Cell::Armor(slot))
}

/// The transaction descriptor for a retained cell, empty for an absent one.
fn verified(held: Option<&Held>) -> Option<VerifiedNetworkItemStack> {
    let stack = held.map_or_else(NetworkItemStack::empty, |held| held.stack.clone());
    let digest = stack.nbt_digest;
    VerifiedNetworkItemStack::try_new(stack, digest).ok()
}
