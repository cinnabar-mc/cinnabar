use super::*;

#[test]
fn server_menu_200_slots_retain_high_cells_without_requesting_close() {
    let mut ledger = open(GENERIC_STORAGE_WINDOW_TYPE);
    let container = descriptor(Some(0), None);
    let mut slots = vec![NetworkItemStack::empty(); 200];
    slots[113] = stack(113);
    slots[199] = stack(199);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container,
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));

    assert_eq!(ledger.storage_slot_count(), Some(200));
    assert!(ledger.pending_batch().unwrap().is_none());
    assert_eq!(ledger.storage_stack(113), Some(&stack(113)));
    assert_eq!(ledger.storage_stack(199), Some(&stack(199)));

    ledger.begin_storage_click(199).unwrap();
    let Some(StackRequestAction::Take { source, .. }) = ledger.newest_action() else {
        panic!("taking the last server menu item");
    };
    assert_eq!(source.slot, 199);
    assert_eq!(source.stack_network_id, 199);
}

const SHOP_CELLS: usize = 549;
const HIGH_CELL: u16 = 508;
/// The cell whose number is `HIGH_CELL`'s one-byte wire slot.
const LOW_TWIN: u16 = 252;

/// A menu past the one-byte range with distinct stacks in a cell and its low-byte twin.
fn shop() -> PlayerInventoryLedger {
    let mut ledger = open(GENERIC_STORAGE_WINDOW_TYPE);
    fill_shop(&mut ledger);
    ledger
}

/// Sends the menu's content to the open window.
fn fill_shop(ledger: &mut PlayerInventoryLedger) {
    let mut slots = vec![NetworkItemStack::empty(); SHOP_CELLS];
    slots[usize::from(LOW_TWIN)] = stack(995);
    slots[usize::from(HIGH_CELL)] = stack(996);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: descriptor(None, None),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
}

/// A normal transaction's slot is a 32-bit int, as `serialize<InventoryAction>::write` sends it.
#[test]
fn client_authoritative_change_names_a_high_cell_in_full() {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Client));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(MENU_WINDOW),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    fill_shop(&mut ledger);
    ledger.begin_storage_click(HIGH_CELL).unwrap();
    let changes = ledger
        .legacy_changes(ledger.queue.back().unwrap(), None)
        .unwrap();
    let slots: Vec<u32> = changes
        .iter()
        .filter(|change| {
            change.source == protocol::NormalInventorySource::Container(MENU_WINDOW as i8)
        })
        .map(|change| change.slot)
        .collect();
    assert_eq!(slots, [u32::from(HIGH_CELL)]);
}

/// Sharing a wire byte with a cell a pending request owns leaves a high cell's own stack id.
#[test]
fn a_pending_low_byte_twin_does_not_claim_the_high_cell() {
    let mut ledger = shop();
    ledger.begin_storage_click(LOW_TWIN).unwrap();
    ledger.begin_drop(DropSource::Cursor, None).unwrap();
    ledger.begin_storage_click(HIGH_CELL).unwrap();
    let mut action = ledger.newest_action().unwrap().clone();
    let ids: Vec<i32> = super::super::helpers::action_slots_mut(&mut action)
        .filter(|slot| matches!(slot.container, StackRequestContainer::LevelEntity { .. }))
        .map(|slot| slot.stack_network_id)
        .collect();
    assert_eq!(ids, [996]);
}

/// A response can only name a cell by its wire byte, which means the cell its request predicted.
#[test]
fn a_low_byte_response_corrects_the_high_cell_its_request_predicted() {
    let mut ledger = shop();
    let request_id = ledger.begin_storage_click(HIGH_CELL).unwrap();
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Response(
        protocol::ItemStackResponseEvent {
            responses: Arc::from([protocol::StackResponse {
                request_id,
                status: protocol::StackResponseStatus::Accepted,
                containers: Arc::from([protocol::StackResponseContainer {
                    container: ContainerIdentity {
                        window_id: None,
                        slot_type: Some(CONTAINER_NAME_LEVEL_ENTITY),
                        dynamic_id: None,
                    },
                    slots: Arc::from([protocol::StackResponseSlot {
                        slot: LOW_TWIN as u8,
                        hotbar_slot: LOW_TWIN as u8,
                        count: 1,
                        item_stack_id: 997,
                        custom_name: Arc::from(""),
                        filtered_custom_name: Arc::from(""),
                        durability_correction: 0,
                    }]),
                }]),
            }]),
        },
    ));
    assert_eq!(ledger.storage_stack(HIGH_CELL), Some(&stack(997)));
    assert_eq!(ledger.storage_stack(LOW_TWIN), Some(&stack(995)));
}
