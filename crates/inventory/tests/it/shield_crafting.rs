//! Shield decoration uses its advertised special recipe and retains input item data.
use inventory::InventorySession;
use protocol::*;
use sha2::Digest;

/// Commits each event at the same FIFO frontier used by live inventory ingress.
fn commit(session: &mut InventorySession, sequence: u64, event: InventoryEvent) {
    session.enqueue_inventory_event(1, sequence, event).unwrap();
    session.synchronize_crafting_frontier(1, Some((1, 0, Some(sequence))));
    session.drain_pending_inventory();
}

/// Installs the real shield and banner identities with a current workbench grid.
fn session() -> (InventorySession, i32) {
    let entries = vanilla_item_registry();
    let identifier = |name: &str| {
        entries
            .iter()
            .find(|entry| entry.identifier.as_ref() == name)
            .unwrap()
            .network_id
    };
    let shield = identifier("minecraft:shield");
    let banner = identifier("minecraft:banner");
    let mut session = InventorySession::new(1);
    for _ in 0..3 {
        session.synchronize_crafting_frontier(1, None);
        session.drain_pending_inventory();
    }
    assert!(session.publish_bootstrap_inventory(
        Some(ItemRegistryEvent { entries }),
        InventoryEvent::Authority(InventoryAuthority::Server)
    ));
    session.publish_crafting_rules_bootstrap(CraftingRules::default());
    commit(
        &mut session,
        2,
        InventoryEvent::Recipes(test_support::recipes::multi_recipe_update(200, 55)),
    );
    commit(
        &mut session,
        3,
        InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(3),
            window_type: inventory::inventory_ledger::WORKBENCH_WINDOW_TYPE,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }),
    );
    let mut slots = vec![NetworkItemStack::empty(); UI_SLOT_COUNT];
    let extra = edit_item_extra(
        &[0; 18],
        &ItemNbtEdit {
            damage: Some(17),
            custom_name: Some(Some("Fixture shield".into())),
            ..Default::default()
        },
    )
    .unwrap();
    slots[32] = NetworkItemStack {
        network_id: shield,
        count: 1,
        stack_network_id: 101,
        nbt_digest: sha2::Sha256::digest(&extra).into(),
        extra_data: extra,
        ..NetworkItemStack::empty()
    };
    slots[33] = NetworkItemStack {
        network_id: banner,
        metadata: 15,
        count: 1,
        stack_network_id: 102,
        ..NetworkItemStack::empty()
    };
    commit(
        &mut session,
        4,
        InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(UI_INVENTORY_WINDOW_ID),
            slots: slots.into(),
            storage_item: NetworkItemStack::empty(),
        }),
    );
    (session, shield)
}

#[test]
fn advertised_banner_recipe_decorates_the_current_shield_without_erasing_its_data() {
    let (mut session, shield) = session();
    assert_eq!(
        session.ledger().crafting_grid(),
        inventory::inventory_ledger::CraftingGrid::Workbench
    );
    let request = session
        .begin_crafting()
        .expect("advertised shield decoration must be craftable");
    let output = session.ledger().cursor_stack().unwrap();
    assert_eq!(output.network_id, shield);
    assert_eq!(item_extra_damage(&output.extra_data), Some(17));
    assert_eq!(
        item_custom_name(&output.extra_data).as_deref(),
        Some("Fixture shield")
    );
    assert!(
        session
            .ledger()
            .target_stack(inventory::inventory_ledger::InventoryTarget::Craft(32))
            .is_none()
    );
    assert!(
        session
            .ledger()
            .target_stack(inventory::inventory_ledger::InventoryTarget::Craft(33))
            .is_none()
    );
    assert!(!output.extra_data.is_empty());
    use test_support::recipes::{
        StackActionObservation as Action, StackSlotObservation as Slot, stack_request_actions,
    };
    let (packet, count) = session.ledger().pending_batch().unwrap().unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        stack_request_actions(&packet).unwrap(),
        vec![
            Action::CraftRecipe {
                recipe_network_id: 55
            },
            Action::CraftResults(vec![Some("minecraft:shield".into())]),
            Action::Consume {
                amount: 1,
                source: Slot {
                    slot: 32,
                    stack_network_id: 101
                }
            },
            Action::Consume {
                amount: 1,
                source: Slot {
                    slot: 33,
                    stack_network_id: 102
                }
            },
            Action::Take {
                amount: 1,
                source: Slot {
                    slot: CREATED_OUTPUT_SLOT,
                    stack_network_id: request
                },
                destination: Slot {
                    slot: 0,
                    stack_network_id: 0
                }
            },
        ]
    );
}

#[test]
fn shield_recipe_removal_retires_the_current_result() {
    let (mut session, _) = session();
    commit(
        &mut session,
        5,
        InventoryEvent::Recipes(
            test_support::recipes::CraftingDataFixture::clearing(vec![]).update(),
        ),
    );
    assert!(matches!(
        session.crafting_match(),
        inventory::CraftGridMatch::NoMatch
    ));
    assert!(session.begin_crafting().is_err());
    assert!(session.ledger().cursor_stack().is_none());
}

#[test]
fn decorated_shield_cannot_consume_another_banner() {
    let (mut session, _) = session();
    let mut shield = session
        .ledger()
        .target_stack(inventory::inventory_ledger::InventoryTarget::Craft(32))
        .unwrap()
        .clone();
    shield.extra_data = apply_banner_to_shield(&shield.extra_data, &[], 15).unwrap();
    shield.nbt_digest = sha2::Sha256::digest(&shield.extra_data).into();
    commit(
        &mut session,
        5,
        InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(UI_INVENTORY_WINDOW_ID),
                slot: 32,
            },
            stack: shield,
            storage_item: None,
        }),
    );
    assert!(matches!(
        session.crafting_match(),
        inventory::CraftGridMatch::NoMatch
    ));
    assert!(session.begin_crafting().is_err());
    assert!(session.ledger().cursor_stack().is_none());
}
