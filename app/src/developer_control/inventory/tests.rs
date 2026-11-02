use std::sync::Arc;

use client_ui::ui_runtime::{UiRuntime, flush_inventory_send};
use developer_control::protocol::{ClickButton, InventoryClick, InventoryContainer};
use protocol::{
    ContainerIdentity, InventoryAuthority, InventoryContentEvent, InventoryEvent, NetworkItemStack,
};
use ui_screens::container_actions::InventoryCellHit;

use super::*;

const SESSION: u64 = 4;
const STONE: &str = "minecraft:stone";

/// A server-authoritative player holding 32 stone in main inventory slot 9.
fn player_with_stone() -> (player_state::PlayerState, UiRuntime) {
    let mut player = player_state::PlayerState::new(SESSION);
    let mut ui = UiRuntime::new(SESSION);
    ui.publish_local_runtime_id(&mut player, SESSION, 42)
        .unwrap();
    let stone = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == STONE)
        .unwrap()
        .clone();
    let ledger = player.inventory.ledger_mut();
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&protocol::ItemRegistryEvent {
        entries: Arc::from([stone.clone()]),
    });
    let mut slots = vec![NetworkItemStack::empty(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)];
    slots[9] = NetworkItemStack {
        network_id: stone.network_id,
        count: 32,
        stack_network_id: 100,
        ..NetworkItemStack::empty()
    };
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::empty(),
    }));
    (player, ui)
}

/// Sends whatever the inventory queued this frame, as the network flush does.
fn flush(player: &mut player_state::PlayerState, ui: &mut UiRuntime) -> usize {
    let mut sent = 0;
    while flush_inventory_send(player, ui, 0, |_| {
        sent += 1;
        Ok::<_, ()>(())
    })
    .unwrap()
    {}
    sent
}

fn click(container: InventoryContainer, slot: u16, button: ClickButton) -> InventoryClick {
    InventoryClick {
        container,
        slot,
        button,
        shift: false,
    }
}

fn count(value: &Value) -> u64 {
    value["count"].as_u64().unwrap()
}

/// Opens like the inventory key, splits a stack and places one item, all predicted before the
/// server answers, then closes like Escape.
#[test]
fn clicks_predict_through_the_player_click_path_without_waiting_for_the_server() {
    let (mut player, mut ui) = player_with_stone();
    let right = |slot| click(InventoryContainer::Inventory, slot, ClickButton::Right);
    assert_eq!(
        click_cell(&mut player, &mut ui, None, right(9)).unwrap_err(),
        "no inventory screen is open; send inventory_open first"
    );

    open_screen(&mut player, &mut ui, None).unwrap();
    assert!(ui.inventory_open());
    assert_eq!(report(&player, &ui)["ready"], false);
    assert!(
        click_cell(&mut player, &mut ui, None, right(9))
            .unwrap_err()
            .starts_with("the client refused the click")
    );
    assert!(flush(&mut player, &mut ui) > 0, "the open request leaves");
    assert_eq!(report(&player, &ui)["ready"], true);
    open_screen(&mut player, &mut ui, None).unwrap();
    assert!(ui.inventory_open(), "opening an open screen keeps it open");

    click_cell(&mut player, &mut ui, None, right(9)).unwrap();
    click_cell(&mut player, &mut ui, None, right(10)).unwrap();
    let view = report(&player, &ui);
    assert_eq!(view["screen"], "inventory");
    assert_eq!(view["cursor"]["identifier"], STONE);
    assert_eq!(count(&view["cursor"]), 15);
    assert_eq!(count(&view["inventory"][9]), 16);
    assert_eq!(count(&view["inventory"][10]), 1);
    assert_eq!(view["inventory"][11], Value::Null);
    assert_eq!(view["inventory"].as_array().unwrap().len(), 36);
    assert_eq!(view["crafting"]["width"], 2);
    assert_eq!(view["crafting"]["input"].as_array().unwrap().len(), 4);
    assert_eq!(view["pending_requests"], 2, "nothing was answered");

    close_screen(&mut player, &mut ui).unwrap();
    assert!(!ui.inventory_open());
    assert_eq!(report(&player, &ui)["open"], false);
    assert_eq!(
        close_screen(&mut player, &mut ui).unwrap_err(),
        "no inventory screen is open"
    );
}

/// Each container resolves to the cell the player's pointer would hit, and only cells the
/// open screen draws are clickable.
#[test]
fn cells_resolve_against_the_open_screen_layout() {
    let (mut player, mut ui) = player_with_stone();
    let ledger = player.inventory.ledger();
    for (container, slot, hit) in [
        (
            InventoryContainer::Inventory,
            0,
            InventoryCellHit::Player(0),
        ),
        (
            InventoryContainer::Inventory,
            35,
            InventoryCellHit::Player(35),
        ),
        (InventoryContainer::Armor, 3, InventoryCellHit::Armor(3)),
        (InventoryContainer::Offhand, 0, InventoryCellHit::Offhand),
        (
            InventoryContainer::Container,
            2,
            InventoryCellHit::Storage(2),
        ),
        (
            InventoryContainer::CraftingInput,
            0,
            InventoryCellHit::Craft(28),
        ),
        (
            InventoryContainer::CraftingInput,
            3,
            InventoryCellHit::Craft(31),
        ),
        (
            InventoryContainer::CraftingOutput,
            0,
            InventoryCellHit::CraftOutput,
        ),
    ] {
        assert_eq!(
            cell_hit(ledger, container, slot),
            Some(hit),
            "{container:?}"
        );
        assert_eq!(container_of(hit), Some(container));
    }
    assert_eq!(cell_hit(ledger, InventoryContainer::Offhand, 1), None);
    assert_eq!(
        cell_hit(ledger, InventoryContainer::CraftingOutput, 1),
        None
    );
    assert_eq!(cell_hit(ledger, InventoryContainer::Inventory, 256), None);

    open_screen(&mut player, &mut ui, None).unwrap();
    flush(&mut player, &mut ui);
    let left = |container, slot| click(container, slot, ClickButton::Left);
    for (container, slot, error) in [
        (
            InventoryContainer::CraftingInput,
            4,
            "crafting_input slot 4 is not on the open inventory screen, which has 4 such slots",
        ),
        (
            InventoryContainer::Inventory,
            36,
            "inventory slot 36 is not on the open inventory screen, which has 36 such slots",
        ),
        (
            InventoryContainer::Container,
            0,
            "container slot 0 is not on the open inventory screen, which has 0 such slots",
        ),
    ] {
        assert_eq!(
            click_cell(&mut player, &mut ui, None, left(container, slot)).unwrap_err(),
            error
        );
    }
    assert_eq!(player.inventory.ledger().pending_request_count(), 0);
}
