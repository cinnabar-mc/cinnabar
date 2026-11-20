//! Right-click armor equip previews and their local settlement.

use std::sync::Arc;

use protocol::{
    ArmorEquipChange, ContainerIdentity, InventoryAuthority, InventoryContentEvent, InventoryEvent,
    ItemComponents, ItemNbtEdit, ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion,
    NetworkItemStack, SlotIdentity, VerifiedNetworkItemStack, edit_item_extra,
};
use sha2::{Digest, Sha256};

use super::*;

const HELMET_ID: i32 = 6;

/// A helmet stack wearing one wire identity.
fn helmet(id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id: HELMET_ID,
        count,
        stack_network_id: id,
        ..NetworkItemStack::default()
    }
}

fn verified(stack: &NetworkItemStack) -> VerifiedNetworkItemStack {
    VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap()
}

/// Enchants a stack through the public full-width editor and refreshes its digest.
fn enchanted(mut stack: NetworkItemStack, enchantment: i16, level: i16) -> NetworkItemStack {
    stack.extra_data = edit_item_extra(
        &stack.extra_data,
        &ItemNbtEdit {
            enchantments: Some(vec![(enchantment, level)]),
            ..Default::default()
        },
    )
    .unwrap();
    stack.nbt_digest = Sha256::digest(&stack.extra_data).into();
    stack
}

/// Gives a stack one item-lock byte, as server-locked items carry on the wire.
fn locked(mut stack: NetworkItemStack, lock: u8) -> NetworkItemStack {
    let key = b"minecraft:item_lock";
    let mut extra = vec![255, 255, 1, 10, 0, 0, 1];
    extra.extend_from_slice(&(key.len() as u16).to_le_bytes());
    extra.extend_from_slice(key);
    extra.push(lock);
    extra.push(0);
    extra.extend_from_slice(&[0; 8]);
    stack.extra_data = extra.into();
    stack.nbt_digest = Sha256::digest(&stack.extra_data).into();
    stack
}

fn binding_helmet(id: i32) -> NetworkItemStack {
    enchanted(
        helmet(id, 1),
        i16::from(protocol_ids::Enchantment::CurseOfBinding.wire_value()),
        1,
    )
}

/// Opens a server-backed ledger whose hotbar holds `held` and whose head slot
/// holds `worn`.
fn open(slot: u8, held: NetworkItemStack, worn: NetworkItemStack) -> Option<PlayerInventoryLedger> {
    open_inner(slot, held, Some(worn))
}

/// Opens the same ledger without any armor container contents.
fn open_unobserved_armor(slot: u8, held: NetworkItemStack) -> Option<PlayerInventoryLedger> {
    open_inner(slot, held, None)
}

fn open_inner(
    slot: u8,
    held: NetworkItemStack,
    worn: Option<NetworkItemStack>,
) -> Option<PlayerInventoryLedger> {
    if assets::facts::ItemFacts::builtin().is_err() {
        eprintln!("missing-fixture: builtin item facts carrier");
        return None;
    }
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&ItemRegistryEvent {
        entries: Arc::from([ItemRegistryEntry {
            identifier: Arc::from("minecraft:diamond_helmet"),
            network_id: HELMET_ID,
            component_based: true,
            version: ItemRegistryVersion::DataDriven,
            components: Some(Arc::new(ItemComponents {
                wearable_declared: true,
                wearable_slot: Some(Arc::from("slot.armor.head")),
                max_stack_size: Some(1),
                ..Default::default()
            })),
            component_digest: [6; 32],
            negotiated_max_stack_size: Some(1),
            canonical_empty_component_data: false,
            item_tags: Arc::from([]),
        }]),
    });
    let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[usize::from(slot)] = held;
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
    if let Some(worn) = worn {
        let mut armor = vec![NetworkItemStack::empty(); usize::from(protocol::ARMOR_SLOTS)];
        armor[0] = worn;
        ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
            slots: armor.into(),
            storage_item: NetworkItemStack::empty(),
        }));
    }
    Some(ledger)
}

fn change(held: &NetworkItemStack, worn: &NetworkItemStack) -> ArmorEquipChange {
    ArmorEquipChange {
        legacy_request_id: -6,
        slot: 3,
        armor_slot: 0,
        from: verified(held),
        to: verified(worn),
    }
}

/// An empty head slot admits the held helmet and the swap settles immediately.
#[test]
fn equipping_into_an_empty_armor_slot_previews_and_settles_the_swap() {
    let held = helmet(101, 1);
    let Some(mut ledger) = open(3, held.clone(), NetworkItemStack::empty()) else {
        return;
    };
    let preview = ledger
        .armor_equip_preview(3, &verified(&held))
        .expect("an empty head slot admits the helmet");
    assert_eq!(preview.armor_slot, 0);
    assert!(preview.worn.is_empty());

    let revision = ledger.authoritative_slot_revision(3).unwrap();
    assert!(ledger.settle_armor_equip(revision, &change(&held, &NetworkItemStack::empty())));
    assert!(ledger.displayed_stack(3).is_none());
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Armor(0))
            .unwrap()
            .stack_network_id,
        101
    );
}

/// A worn piece moves back to the source slot, preserving both wire identities.
#[test]
fn equipping_replaces_worn_armor_and_returns_it_to_the_source_slot() {
    let held = helmet(101, 1);
    let worn = helmet(202, 1);
    let Some(mut ledger) = open(3, held.clone(), worn.clone()) else {
        return;
    };
    let preview = ledger
        .armor_equip_preview(3, &verified(&held))
        .expect("the worn helmet is replaceable");
    assert_eq!(preview.worn, verified(&worn));

    let revision = ledger.authoritative_slot_revision(3).unwrap();
    assert!(ledger.settle_armor_equip(revision, &change(&held, &worn)));
    assert_eq!(ledger.displayed_stack(3).unwrap().stack_network_id, 202);
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Armor(0))
            .unwrap()
            .stack_network_id,
        101
    );
}

/// Curse of Binding blocks the worn piece's removal outside creative only.
#[test]
fn binding_worn_armor_is_refused_outside_creative_and_allowed_inside() {
    let held = helmet(101, 1);
    let Some(mut ledger) = open(3, held.clone(), binding_helmet(202)) else {
        return;
    };
    assert!(ledger.armor_equip_preview(3, &verified(&held)).is_none());
    ledger.set_permissions(InventoryPermissions {
        creative: true,
        instabuild: None,
    });
    assert_eq!(
        ledger
            .armor_equip_preview(3, &verified(&held))
            .expect("creative ignores Curse of Binding")
            .worn
            .count(),
        1
    );
}

/// A mode-1 slot lock refuses the swap from either side.
#[test]
fn slot_locked_armor_or_held_item_refuses_the_preview() {
    let held = helmet(101, 1);
    let locked_held = locked(held.clone(), 1);
    let Some(locked_source) = open(3, locked_held.clone(), NetworkItemStack::empty()) else {
        return;
    };
    assert!(
        locked_source
            .armor_equip_preview(3, &verified(&locked_held))
            .is_none()
    );
    let Some(locked_worn) = open(3, held.clone(), locked(helmet(202, 1), 1)) else {
        return;
    };
    assert!(
        locked_worn
            .armor_equip_preview(3, &verified(&held))
            .is_none()
    );
}

/// A mode-2 inventory lock leaves the equip admitted by the ledger rules.
/// Native confirmation of this case is still pending BDS evidence.
#[test]
fn inventory_lock_mode_two_keeps_the_equip_admitted() {
    let held = locked(helmet(101, 1), 2);
    let Some(ledger) = open(3, held.clone(), NetworkItemStack::empty()) else {
        return;
    };
    assert!(ledger.armor_equip_preview(3, &verified(&held)).is_some());
}

/// Armor placement admits exactly one item, so an oversized stack is refused.
#[test]
fn stacks_above_one_are_refused() {
    let held = helmet(101, 2);
    let Some(ledger) = open(3, held.clone(), NetworkItemStack::empty()) else {
        return;
    };
    assert!(ledger.armor_equip_preview(3, &verified(&held)).is_none());
}

/// Previewing waits for the armor container's contents and the exact held stack.
#[test]
fn unobserved_armor_or_a_changed_source_refuses_the_preview() {
    let held = helmet(101, 1);
    let Some(ledger) = open(3, held.clone(), NetworkItemStack::empty()) else {
        return;
    };
    assert!(ledger.armor_equip_preview(3, &verified(&held)).is_some());
    assert!(
        ledger
            .armor_equip_preview(3, &verified(&helmet(102, 1)))
            .is_none(),
        "the source cell holds another stack"
    );

    let Some(unobserved) = open_unobserved_armor(3, held.clone()) else {
        return;
    };
    assert!(
        unobserved
            .armor_equip_preview(3, &verified(&held))
            .is_none(),
        "armor contents have not arrived"
    );
    assert_eq!(
        unobserved.armor_equip_slot(&verified(&held)),
        Some(0),
        "the destination is known even when the swap cannot be predicted"
    );
}

/// Settlement revalidates the revision, the exact stacks and the legacy request.
#[test]
fn settle_refuses_stale_revisions_and_mismatched_stacks() {
    let held = helmet(101, 1);
    let worn = helmet(202, 1);
    let Some(mut ledger) = open(3, held.clone(), worn.clone()) else {
        return;
    };
    let revision = ledger.authoritative_slot_revision(3).unwrap();
    assert!(!ledger.settle_armor_equip(revision + 1, &change(&held, &worn)));
    assert_eq!(ledger.displayed_stack(3).unwrap().stack_network_id, 101);

    let mut other_from = change(&held, &worn);
    other_from.from = verified(&helmet(103, 1));
    assert!(!ledger.settle_armor_equip(revision, &other_from));

    let mut other_to = change(&held, &worn);
    other_to.to = verified(&helmet(203, 1));
    assert!(!ledger.settle_armor_equip(revision, &other_to));

    let mut other_slot = change(&held, &worn);
    other_slot.armor_slot = 1;
    assert!(!ledger.settle_armor_equip(revision, &other_slot));

    let mut bad_legacy = change(&held, &worn);
    bad_legacy.legacy_request_id = -5;
    assert!(!ledger.settle_armor_equip(revision, &bad_legacy));
    assert_eq!(ledger.displayed_stack(3).unwrap().stack_network_id, 101);
}

/// A later authoritative restatement replaces the local swap, and a matching
/// one keeps it, as the server repeats the equip.
#[test]
fn authoritative_restatements_correct_the_local_swap() {
    let held = helmet(101, 1);
    let worn = helmet(202, 1);
    let Some(mut ledger) = open(3, held.clone(), worn.clone()) else {
        return;
    };
    let revision = ledger.authoritative_slot_revision(3).unwrap();
    assert!(ledger.settle_armor_equip(revision, &change(&held, &worn)));

    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 3,
        },
        stack: held.clone(),
        storage_item: None,
    }));
    assert_eq!(ledger.displayed_stack(3).unwrap().stack_network_id, 101);
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
            slot: 0,
        },
        stack: worn.clone(),
        storage_item: None,
    }));
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Armor(0))
            .unwrap()
            .stack_network_id,
        202
    );

    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 3,
        },
        stack: worn.clone(),
        storage_item: None,
    }));
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
            slot: 0,
        },
        stack: held.clone(),
        storage_item: None,
    }));
    assert_eq!(ledger.displayed_stack(3).unwrap().stack_network_id, 202);
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Armor(0))
            .unwrap()
            .stack_network_id,
        101
    );
}
