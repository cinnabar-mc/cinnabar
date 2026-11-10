use super::*;
use crate::{
    mining::FrozenMiningSelection,
    movement::{
        PhysicsCorrectionOutcome, flush_player_auth_inputs, physics::PhysicsCorrectionPlan,
    },
    survival_mining::{DestroyTarget, MiningAdmission, SurvivalTickPayload},
    test_support::survival_mining::{completed, evidence, ticker_with_ticks},
};
use inventory::PlayerInventoryLedger;
use protocol::{InventoryAuthority, InventoryEvent, NetworkItemStack, VerifiedNetworkItemStack};

/// Starts with one known tool, without requiring a local item carrier.
fn inventory(authority: InventoryAuthority) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(7);
    ledger.apply(&InventoryEvent::Authority(authority));
    replace_tool(&mut ledger, 12);
    ledger
}

/// Restates a fresh tool in the selected slot with the server's stack name.
fn replace_tool(ledger: &mut PlayerInventoryLedger, stack_network_id: i32) {
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: protocol::ContainerIdentity::window(0),
            slot: 0,
        },
        stack: NetworkItemStack {
            network_id: 300,
            count: 1,
            stack_network_id,
            ..NetworkItemStack::empty()
        },
        storage_item: None,
    }));
}

/// Restates the complete player inventory, holding a fresh tool with the server's stack name.
fn restate_inventory(ledger: &mut PlayerInventoryLedger, stack_network_id: i32) {
    let mut slots = vec![NetworkItemStack::empty(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)];
    slots[0] = NetworkItemStack {
        network_id: 300,
        count: 1,
        stack_network_id,
        ..NetworkItemStack::empty()
    };
    ledger.apply(&InventoryEvent::Content(protocol::InventoryContentEvent {
        container: protocol::ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
}

/// Admits an exact tool change to an unsent movement tick and commits its inventory effect.
fn attach_wear(
    ticker: &mut MovementTicker,
    ledger: &mut PlayerInventoryLedger,
    tick: u64,
    damage: i32,
) {
    let stack = ledger.backing_stack(0).unwrap().clone();
    let from = VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap();
    let legacy_request_id = protocol::next_legacy_request_id().unwrap();
    let payload = SurvivalTickPayload {
        destroy: Some(DestroyTarget {
            local_runtime_id: 8,
            position: [1, 2, 3],
            face: 1,
            runtime_id: 9,
            wire_runtime_id: 9,
            relative_hit: [0.5; 3],
            block: None,
            conditions: Default::default(),
            selection: FrozenMiningSelection {
                slot: 0,
                item: from.clone(),
            },
            wear: None,
            instant: false,
        }),
        destroy_change: Some(protocol::PredictedSlotChange {
            to: from.worn(damage, Some(250), legacy_request_id).unwrap(),
            from,
            legacy_request_id,
        }),
        ..Default::default()
    };
    let mut admitted = MiningAdmission::default();
    assert!(payload.attach(ticker, tick, &mut admitted));
    admitted.settle_inventory(ledger);
}

#[test]
fn revoked_backpressured_carriers_restore_worn_and_broken_tools() {
    for authority in [InventoryAuthority::Client, InventoryAuthority::Server] {
        for damage in [6, 251] {
            let mut ticker = ticker_with_ticks(2);
            let mut ledger = inventory(authority);
            let original = ledger.backing_stack(0).unwrap().clone();
            attach_wear(&mut ticker, &mut ledger, 101, 4);
            attach_wear(&mut ticker, &mut ledger, 102, damage);
            assert!(
                flush_player_auth_inputs(&mut ticker, 2, Some(evidence()), |_, _| Err("full"))
                    .is_err()
            );
            ticker.reanchor_surface_spawn(102, completed(102).position);
            ticker
                .take_revoked_mining()
                .unwrap()
                .settle_inventory(&mut ledger);
            assert_eq!(ledger.backing_stack(0), Some(&original));
            assert_eq!(ledger.displayed_stack(0), Some(&original));
            assert!(!ledger.resync_required());
            assert!(ticker.take_revoked_mining().is_none());
        }
    }
}

#[test]
fn a_replay_recovers_wear_before_discarding_its_carrier_tick() {
    let mut ticker = ticker_with_ticks(2);
    let mut ledger = inventory(InventoryAuthority::Server);
    let original = ledger.backing_stack(0).unwrap().clone();
    attach_wear(&mut ticker, &mut ledger, 101, 251);
    ticker
        .apply_correction_plan(&PhysicsCorrectionPlan {
            outcome: PhysicsCorrectionOutcome::Replayed {
                corrected_tick: 101,
                replayed_ticks: 1,
            },
            corrected_tick: 101,
            final_tick: 102,
            final_position: completed(102).position,
            anchor_input: Default::default(),
            corrected_sample: Some(completed(101)),
            replayed_samples: vec![completed(102)],
        })
        .unwrap();
    ticker
        .take_revoked_mining()
        .unwrap()
        .settle_inventory(&mut ledger);
    assert_eq!(ledger.displayed_stack(0), Some(&original));
    flush_player_auth_inputs(&mut ticker, 2, Some(evidence()), |_, packet| {
        assert!(protocol::test_support::transactions::observe_input_transaction(&packet).is_none());
        Ok::<_, ()>(())
    })
    .unwrap();
}

#[test]
fn revoked_socket_pending_wear_requires_recovery_without_guessing_delivery() {
    let mut ticker = ticker_with_ticks(1);
    let mut ledger = inventory(InventoryAuthority::Server);
    attach_wear(&mut ticker, &mut ledger, 101, 6);
    let mut sent = None;
    flush_player_auth_inputs(&mut ticker, 1, Some(evidence()), |identity, _| {
        sent = Some(identity);
        Ok::<_, ()>(())
    })
    .unwrap();
    ticker.reanchor_surface_spawn(101, completed(101).position);
    ticker
        .take_revoked_mining()
        .unwrap()
        .settle_inventory(&mut ledger);
    let unverified = ledger.backing_stack(0).unwrap();
    assert_eq!(protocol::item_extra_damage(&unverified.extra_data), Some(6));
    assert_eq!(unverified.stack_network_id, 0);
    assert!(ledger.resync_required());
    assert!(ticker.acknowledge_physics_send(sent.unwrap()));
    // One slot update leaves the rest of the player surface unverified.
    replace_tool(&mut ledger, 77);
    assert_eq!(ledger.backing_stack(0).unwrap().stack_network_id, 77);
    assert!(ledger.resync_required());
    restate_inventory(&mut ledger, 78);
    assert_eq!(ledger.backing_stack(0).unwrap().stack_network_id, 78);
    assert!(!ledger.resync_required());
}

#[test]
fn revocation_preserves_written_wear_and_authoritative_replacements() {
    for written in [false, true] {
        let mut ticker = ticker_with_ticks(1);
        let mut ledger = inventory(InventoryAuthority::Server);
        attach_wear(&mut ticker, &mut ledger, 101, 6);
        if written {
            let mut sent = None;
            flush_player_auth_inputs(&mut ticker, 1, Some(evidence()), |identity, _| {
                sent = Some(identity);
                Ok::<_, ()>(())
            })
            .unwrap();
            assert!(ticker.acknowledge_physics_send(sent.unwrap()));
        } else {
            replace_tool(&mut ledger, 77);
        }
        let current = ledger.backing_stack(0).unwrap().clone();
        ticker.reanchor_surface_spawn(101, completed(101).position);
        if let Some(revoked) = ticker.take_revoked_mining() {
            revoked.settle_inventory(&mut ledger);
        }
        assert_eq!(ledger.displayed_stack(0), Some(&current));
        assert!(!ledger.resync_required());
    }
}
