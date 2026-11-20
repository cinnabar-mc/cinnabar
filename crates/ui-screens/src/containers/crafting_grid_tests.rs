//! The crafting grid shows each occupied cell's durability bar.

use std::collections::HashMap;

use json_ui::Scalar;
use protocol::NetworkItemStack;

use super::{Cells, crafting_grid_cells, crafting_output_cell};
use crate::container_source::ContainerFrame;
use crate::inventory_capture::CraftingFrame;

/// The first UI slot of the personal 2x2 grid.
const FIRST_SLOT: u8 = 28;

/// Writes `stack` into UI slot `slot`, as the server does for a crafting input.
fn put(player: &mut player_state::PlayerState, slot: u8, stack: NetworkItemStack) {
    player
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Slot(
            protocol::InventorySlotEvent {
                identity: protocol::SlotIdentity {
                    container: protocol::ContainerIdentity::window(
                        protocol::UI_INVENTORY_WINDOW_ID,
                    ),
                    slot: u16::from(slot),
                },
                stack,
                storage_item: None,
            },
        ));
}

/// A tool stack: only its network id and count matter to the grid.
fn tool(network_id: i32) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        count: 1,
        ..NetworkItemStack::empty()
    }
}

/// Runs `check` on the 2x2 grid's cells for a frame whose UI durability bars are `bars`.
fn with_grid(bars: &[(u8, f32)], occupied: &[u8], check: impl FnOnce(&[json_ui::CollectionItem])) {
    let mut player = player_state::PlayerState::new(1);
    player
        .inventory
        .publish_inventory_authority(protocol::InventoryAuthority::Server);
    for &slot in occupied {
        put(&mut player, slot, tool(1));
    }
    let names = HashMap::new();
    let (icons36, durability36, durability9) = ([None; 36], [None; 36], [None; 9]);
    let (icons54, durability54) = ([None; 54], [None; 54]);
    let (icons4, durability4) = ([None; 4], [None; 4]);
    let mut ui_durability = [None; protocol::CREATED_OUTPUT_SLOT as usize + 1];
    for &(slot, fraction) in bars {
        ui_durability[usize::from(slot)] = Some(fraction);
    }
    let entries: std::sync::Arc<[Option<ui::IconRef>]> = std::sync::Arc::from([]);
    let frame = ContainerFrame {
        item_names: &names,
        inventory_icons: &icons36,
        player_durability: &durability36,
        hotbar_durability: &durability9,
        storage_icons: &icons54,
        storage_durability: &durability54,
        ui_icons: &[],
        ui_durability: &ui_durability,
        crafting: &CraftingFrame::default(),
        armor_icons: &icons4,
        armor_durability: &durability4,
        offhand_icon: None,
        offhand_durability: None,
        cursor_icon: None,
        cursor_durability: None,
        recipe_icons: &[],
        recipe_output: None,
        book_entries: &[],
        furnace_entries: &entries,
    };
    let ledger = player.inventory.ledger();
    let mut icons = Vec::new();
    let mut cells = Cells {
        names: &names,
        facts: ledger.item_facts(),
        icons: &mut icons,
    };
    check(&crafting_grid_cells(
        &mut cells, ledger, &frame, FIRST_SLOT, 2,
    ));
}

/// Whether a cell draws a durability bar, and how full it is out of 1000.
fn bar(cell: &json_ui::CollectionItem) -> (bool, f64) {
    let Scalar::Bool(visible) = cell.values["#item_durability_visible"] else {
        panic!("the bar's visibility is a boolean");
    };
    let Scalar::Num(current) = cell.values["#item_durability_current_amount"] else {
        panic!("the bar's fill is a number");
    };
    (visible, current)
}

/// Two damaged tools in the grid keep the bars they show in the hotbar.
#[test]
fn damaged_tools_in_the_grid_show_their_durability_bars() {
    with_grid(
        &[(FIRST_SLOT, 0.5), (FIRST_SLOT + 1, 0.25)],
        &[FIRST_SLOT, FIRST_SLOT + 1],
        |grid| {
            assert_eq!(bar(&grid[0]), (true, 500.0));
            assert_eq!(bar(&grid[1]), (true, 250.0));
        },
    );
}

/// An undamaged tool and an empty cell draw no bar.
#[test]
fn undamaged_and_empty_grid_cells_show_no_bar() {
    with_grid(&[], &[FIRST_SLOT], |grid| {
        assert!(!bar(&grid[0]).0);
        assert!(!bar(&grid[3]).0);
    });
}

/// A bar belongs to its own cell, never the cell beside it.
#[test]
fn each_grid_cell_reads_its_own_bar() {
    with_grid(
        &[(FIRST_SLOT + 1, 0.75)],
        &[FIRST_SLOT, FIRST_SLOT + 1],
        |grid| {
            assert!(!bar(&grid[0]).0);
            assert_eq!(bar(&grid[1]), (true, 750.0));
        },
    );
}

/// Runs `check` on the result cell of a frame showing `output`, with the server's created
/// output (if any) already written to the ledger.
fn with_output(
    created: Option<NetworkItemStack>,
    output: Option<(NetworkItemStack, bool)>,
    check: impl FnOnce(&json_ui::CollectionItem),
) {
    let mut player = player_state::PlayerState::new(1);
    player
        .inventory
        .publish_inventory_authority(protocol::InventoryAuthority::Server);
    if let Some(stack) = created {
        put(&mut player, protocol::CREATED_OUTPUT_SLOT, stack);
    }
    let names = HashMap::new();
    let (icons36, durability36, durability9) = ([None; 36], [None; 36], [None; 9]);
    let (icons54, durability54) = ([None; 54], [None; 54]);
    let (icons4, durability4) = ([None; 4], [None; 4]);
    let mut ui_durability = [None; protocol::CREATED_OUTPUT_SLOT as usize + 1];
    ui_durability[usize::from(protocol::CREATED_OUTPUT_SLOT)] = Some(0.5);
    let crafting = CraftingFrame {
        output: output.clone().map(|(stack, _)| (None, stack)),
        output_is_ghost: output.is_some_and(|(_, ghost)| ghost),
        ..Default::default()
    };
    let entries: std::sync::Arc<[Option<ui::IconRef>]> = std::sync::Arc::from([]);
    let frame = ContainerFrame {
        item_names: &names,
        inventory_icons: &icons36,
        player_durability: &durability36,
        hotbar_durability: &durability9,
        storage_icons: &icons54,
        storage_durability: &durability54,
        ui_icons: &[],
        ui_durability: &ui_durability,
        crafting: &crafting,
        armor_icons: &icons4,
        armor_durability: &durability4,
        offhand_icon: None,
        offhand_durability: None,
        cursor_icon: None,
        cursor_durability: None,
        recipe_icons: &[],
        recipe_output: None,
        book_entries: &[],
        furnace_entries: &entries,
    };
    let ledger = player.inventory.ledger();
    let mut icons = Vec::new();
    let mut cells = Cells {
        names: &names,
        facts: ledger.item_facts(),
        icons: &mut icons,
    };
    check(&crafting_output_cell(&mut cells, ledger, &frame));
}

/// A result the server created shows that stack's durability bar.
#[test]
fn a_created_result_shows_its_durability_bar() {
    with_output(Some(tool(1)), Some((tool(1), false)), |cell| {
        assert_eq!(bar(cell), (true, 500.0))
    });
}

/// A predicted result and a recipe-book ghost have no server stack, so they draw no bar.
#[test]
fn predicted_and_ghost_results_show_no_bar() {
    with_output(None, Some((tool(1), false)), |cell| assert!(!bar(cell).0));
    with_output(None, Some((tool(1), true)), |cell| assert!(!bar(cell).0));
    with_output(None, None, |cell| assert!(!bar(cell).0));
}
