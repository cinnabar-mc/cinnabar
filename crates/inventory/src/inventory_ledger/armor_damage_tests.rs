//! `PlayerArmorDamage` rewrites worn armor durability in place.

use std::sync::Arc;

use protocol::{
    ArmorDamage, ArmorDamageEvent, ContainerIdentity, InventoryAuthority, InventoryEvent,
    InventorySlotEvent, NetworkItemStack, SlotIdentity,
};

use super::{InventoryTarget, PlayerInventoryLedger};

const CHESTPLATE_STACK_ID: i32 = 41;

/// An undamaged chestplate as servers send it: no compound, empty trailing lists.
fn chestplate() -> NetworkItemStack {
    NetworkItemStack {
        network_id: 347,
        stack_network_id: CHESTPLATE_STACK_ID,
        count: 1,
        ..NetworkItemStack::empty()
    }
}

/// A server-authoritative ledger wearing [`chestplate`].
fn wearing_chestplate() -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
            slot: 1,
        },
        stack: chestplate(),
        storage_item: None,
    }));
    ledger
}

fn damage(pieces: &[(i32, i16)]) -> InventoryEvent {
    InventoryEvent::ArmorDamage(ArmorDamageEvent {
        pieces: pieces
            .iter()
            .map(|&(slot, damage)| ArmorDamage { slot, damage })
            .collect::<Arc<[_]>>(),
    })
}

fn worn_damage(ledger: &PlayerInventoryLedger, slot: u8) -> Option<u32> {
    protocol::item_stack_damage(ledger.target_stack(InventoryTarget::Armor(slot))?)
}

#[test]
fn armor_damage_sets_the_worn_piece_damage_and_keeps_its_identity() {
    let mut ledger = wearing_chestplate();
    assert_eq!(worn_damage(&ledger, 1), None);

    ledger.apply(&damage(&[(1, 12)]));
    assert_eq!(worn_damage(&ledger, 1), Some(12));
    // Each packet states the absolute damage rather than an amount to add.
    ledger.apply(&damage(&[(1, 13)]));
    assert_eq!(worn_damage(&ledger, 1), Some(13));

    let worn = ledger.target_stack(InventoryTarget::Armor(1)).unwrap();
    assert_eq!(worn.stack_network_id, CHESTPLATE_STACK_ID);
    assert_eq!((worn.network_id, worn.count), (347, 1));
}

#[test]
fn armor_damage_skips_empty_and_unknown_slots() {
    let mut ledger = wearing_chestplate();
    ledger.apply(&damage(&[(0, 5), (7, 5), (-1, 5), (1, 2)]));
    assert_eq!(ledger.target_stack(InventoryTarget::Armor(0)), None);
    assert_eq!(worn_damage(&ledger, 1), Some(2));
}

#[test]
fn armor_damage_needs_server_authoritative_inventories() {
    let mut ledger = wearing_chestplate();
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Client));
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
            slot: 1,
        },
        stack: chestplate(),
        storage_item: None,
    }));
    ledger.apply(&damage(&[(1, 12)]));
    assert_eq!(worn_damage(&ledger, 1), None);
}
