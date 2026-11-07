//! Worn armor durability from `PlayerArmorDamage`.
//!
//! The server reports each damaged piece's new absolute damage instead of restating the
//! armor slot. The worn stack is rewritten in place: its identity, stack network ID and
//! other item data stay, and only its `Damage` changes.

use sha2::{Digest, Sha256};

use protocol::{ArmorDamageEvent, InventoryAuthority};

use super::cells::ARMOR_CELLS;
use super::{Cell, PlayerInventoryLedger};

impl PlayerInventoryLedger {
    /// Applies each piece's new damage to the worn armor cell it names. Without
    /// server-authoritative inventories the packet is not expected and is skipped, as are
    /// slots outside the armor surface; an empty slot stays empty.
    pub(super) fn apply_armor_damage(&mut self, event: &ArmorDamageEvent) {
        if self.authority != Some(InventoryAuthority::Server) {
            tracing::warn!(target: "cinnabar::inventory_requests",
                "skipping armor damage without server-authoritative inventories");
            self.note_unrouted_container();
            return;
        }
        for piece in event.pieces.iter() {
            let Some(slot) = u8::try_from(piece.slot)
                .ok()
                .filter(|slot| usize::from(*slot) < ARMOR_CELLS)
            else {
                tracing::warn!(target: "cinnabar::inventory_requests",
                    slot = piece.slot, "skipping armor damage for an unknown slot");
                self.note_unrouted_container();
                continue;
            };
            let cell = Cell::Armor(slot);
            let Some(mut held) = self.confirmed.get(cell).cloned() else {
                continue;
            };
            let damage = i32::from(piece.damage);
            let Some(extra_data) = protocol::edit_item_extra(
                &held.stack.extra_data,
                &protocol::ItemNbtEdit {
                    damage: Some(damage),
                    ..Default::default()
                },
            )
            .filter(|extra| extra.len() <= protocol::MAX_ITEM_EXTRA_BYTES) else {
                tracing::warn!(target: "cinnabar::inventory_requests",
                    slot, "skipping armor damage for unreadable item data");
                continue;
            };
            held.stack.nbt_digest = Sha256::digest(&extra_data).into();
            held.stack.extra_data = extra_data;
            // The rewritten tag is now the piece's authoritative damage.
            if let Some(overlay) = held.overlay.as_mut() {
                overlay.durability_correction = None;
            }
            self.set_authoritative_cell(cell, Some(held));
        }
    }
}
