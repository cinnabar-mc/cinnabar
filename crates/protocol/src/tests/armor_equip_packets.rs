//! Click-air armor equip transactions.

use std::sync::Arc;

use bedrock_protocol_1_26_60::enums::{ContainerEnumName, InventorySourceType};
use bedrock_protocol_1_26_60::types::InventoryTransactionPacketData;
use bedrock_protocol_1_26_60::wire;
use protocol::{
    ArmorEquipChange, BlockUsePacketError, ItemUseHand, NetworkItemStack, VerifiedNetworkItemStack,
    click_air_equip_packet,
};
use sha2::{Digest, Sha256};

use super::interaction_packets::{decoded_inventory_packet, held_request};

fn armor(id: i32) -> VerifiedNetworkItemStack {
    let extra_data: Arc<[u8]> = Arc::from([]);
    let digest: [u8; 32] = Sha256::digest(&extra_data).into();
    VerifiedNetworkItemStack::try_new(
        NetworkItemStack {
            network_id: 6,
            metadata: 0,
            stack_network_id: id,
            count: 1,
            nbt_digest: digest,
            block_runtime_id: 0,
            extra_data,
        },
        digest,
    )
    .unwrap()
}

/// An armor equip reports the worn piece as the source slot's new stack and
/// names the destination armor slot in its legacy request, matching vanilla.
#[test]
fn click_air_equip_carries_the_swap_and_the_armor_legacy_slot() {
    let from = armor(57);
    let to = armor(58);
    let mut request = held_request();
    request.selected_item = from.clone();
    let built = decoded_inventory_packet(
        click_air_equip_packet(
            request,
            ArmorEquipChange {
                legacy_request_id: -6,
                slot: 3,
                armor_slot: 0,
                from,
                to,
            },
        )
        .unwrap(),
    );
    assert_eq!(built.legacy_request_id.0, -6);
    let slots = built.legacy_set_item_slots.expect("legacy set slots");
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].container_enum, ContainerEnumName::ArmorContainer);
    assert_eq!(slots[0].slots.as_ref(), [0]);
    let InventoryTransactionPacketData::ItemUseInventoryTransaction(transaction) =
        built.transaction
    else {
        panic!("item use");
    };
    assert_eq!(transaction.actions.actions.len(), 1);
    let action = &transaction.actions.actions[0];
    assert_eq!(
        action.source.source_type,
        InventorySourceType::ContainerInventory
    );
    assert_eq!(action.source.container_id, Some(wire::I8(0)));
    assert_eq!(action.slot.0, 3);
    assert_eq!(action.from_item.stack_size.0, 1);
    assert_eq!(action.to_item.stack_size.0, 1);
    assert_eq!(action.to_item.net_id_variant, None);
    assert_eq!(transaction.item.stack_size.0, 1);
}

/// An empty destination still stamps the legacy request, unlike an emptied throw,
/// because the named slot is the armor cell rather than the source.
#[test]
fn click_air_equip_onto_an_empty_armor_slot_keeps_the_legacy_request() {
    let from = armor(57);
    let mut request = held_request();
    request.selected_item = from.clone();
    let built = decoded_inventory_packet(
        click_air_equip_packet(
            request,
            ArmorEquipChange {
                legacy_request_id: -8,
                slot: 3,
                armor_slot: 0,
                from,
                to: VerifiedNetworkItemStack::try_new(
                    NetworkItemStack::empty(),
                    NetworkItemStack::empty().nbt_digest,
                )
                .unwrap(),
            },
        )
        .unwrap(),
    );
    assert_eq!(built.legacy_request_id.0, -8);
    let slots = built.legacy_set_item_slots.expect("legacy set slots");
    assert_eq!(slots[0].container_enum, ContainerEnumName::ArmorContainer);
    let InventoryTransactionPacketData::ItemUseInventoryTransaction(transaction) =
        built.transaction
    else {
        panic!("item use");
    };
    let action = &transaction.actions.actions[0];
    assert_eq!(action.to_item.id.0, 0);
    assert_eq!(action.to_item.stack_size.0, 0);
}

/// The builder only accepts main-hand swaps that name the request's own source
/// cell, a real armor slot and a client legacy request id.
#[test]
fn click_air_equip_refuses_inconsistent_requests() {
    let from = armor(57);
    let change = ArmorEquipChange {
        legacy_request_id: -6,
        slot: 3,
        armor_slot: 0,
        from: from.clone(),
        to: armor(58),
    };
    let mut request = held_request();
    request.selected_item = from.clone();
    assert_eq!(
        click_air_equip_packet(
            request.clone(),
            ArmorEquipChange {
                slot: 4,
                ..change.clone()
            }
        ),
        Err(BlockUsePacketError::ArmorEquipMismatch)
    );
    assert_eq!(
        click_air_equip_packet(
            request.clone(),
            ArmorEquipChange {
                from: armor(59),
                ..change.clone()
            }
        ),
        Err(BlockUsePacketError::ArmorEquipMismatch)
    );
    let mut offhand = request.clone();
    offhand.hand = ItemUseHand::Offhand;
    assert_eq!(
        click_air_equip_packet(offhand, change.clone()),
        Err(BlockUsePacketError::ArmorEquipMismatch)
    );
    assert_eq!(
        click_air_equip_packet(
            request.clone(),
            ArmorEquipChange {
                armor_slot: protocol::ARMOR_SLOTS,
                ..change.clone()
            }
        ),
        Err(BlockUsePacketError::InvalidArmorSlot(protocol::ARMOR_SLOTS))
    );
    for legacy_request_id in [0, -5] {
        assert_eq!(
            click_air_equip_packet(
                request.clone(),
                ArmorEquipChange {
                    legacy_request_id,
                    ..change.clone()
                }
            ),
            Err(BlockUsePacketError::InvalidLegacyRequestId(
                legacy_request_id
            ))
        );
    }
}
