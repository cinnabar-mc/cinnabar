//! Inventory screen control: opens, clicks and closes the screen through the same UI runtime
//! calls the player's inventory key, pointer and Escape make, and reports the client's own
//! view of its items. Clicks are predicted and queued locally; nothing waits for the server.

use bevy::prelude::*;
use client_shell::ui_resources::UiRuntimeResource;
use client_ui::ui_runtime::{
    UiRuntime,
    inventory_drag::PointerAction,
    presentation::{inventory_pointer::InventoryScreen, screens},
};
use developer_control::{
    protocol::{ClickButton, Condition, InventoryClick, InventoryContainer},
    server::Reply,
};
use inventory::inventory_ledger::{InventoryTarget, PlayerInventoryLedger};
use protocol::NetworkItemStack;
use serde_json::{Value, json};
use ui_screens::container_actions::InventoryCellHit;

use crate::runtime::world::ClientWorld;

/// How long `inventory_open` waits for the screen to accept clicks.
const OPEN_WAIT_MS: u64 = 5_000;

/// Opens the player's inventory, replying once the screen accepts clicks.
pub(super) fn open(world: &mut World, reply: Reply) {
    let opened = with_inventory(world, |player, ui, world| {
        let stream = world
            .get_resource::<ClientWorld>()
            .and_then(|world| world.stream.as_ref())
            .ok_or("the player is not in a world")?;
        open_screen(player, ui, Some(stream.authority().inventory_ui()))
    });
    match opened {
        Ok(()) => super::state::wait(world, Condition::InventoryReady, Some(OPEN_WAIT_MS), reply),
        Err(error) => reply.send(Err(error)),
    }
}

/// Closes the open screen as Escape does and reports the client's view right after.
pub(super) fn close(world: &mut World) -> Result<Value, String> {
    with_inventory(world, |player, ui, _| close_screen(player, ui))?;
    if let Some(mut focus) = world.get_resource_mut::<client_state::CursorFocus>() {
        focus.authorize_screen_return();
    }
    Ok(json!({ "closed": true, "inventory": snapshot(world) }))
}

/// Clicks one cell and reports the predicted result.
pub(super) fn click(world: &mut World, command: InventoryClick) -> Result<Value, String> {
    let request_id = with_inventory(world, |player, ui, _| click_cell(player, ui, command))?;
    Ok(json!({ "request_id": request_id, "inventory": snapshot(world) }))
}

/// The client's inventory view, or null before the UI runtime exists.
pub(super) fn snapshot(world: &World) -> Value {
    match (
        world.get_resource::<client_state::PlayerRuntime>(),
        world.get_resource::<UiRuntimeResource>(),
    ) {
        (Some(player), Some(ui)) => report(player, ui),
        _ => Value::Null,
    }
}

/// `Ok` with the inventory view when `condition` holds, else `Err` with what is missing.
pub(super) fn evaluate(world: &World, condition: &Condition) -> Result<Value, String> {
    let (Some(player), Some(ui)) = (
        world.get_resource::<client_state::PlayerRuntime>(),
        world.get_resource::<UiRuntimeResource>(),
    ) else {
        return Err("no inventory yet".into());
    };
    let ledger = ui.inventory_ledger(player);
    match condition {
        Condition::InventoryReady if !ui.inventory_open() => {
            Err("no inventory screen is open".into())
        }
        Condition::InventoryReady if !ledger.screen_accepts_gestures() => {
            Err("the inventory screen is still opening".into())
        }
        Condition::InventorySettled if ledger.pending_request_count() != 0 => Err(format!(
            "{} inventory requests are waiting for the server",
            ledger.pending_request_count()
        )),
        _ => Ok(report(player, ui)),
    }
}

/// Lends the player and UI runtime, with the rest of the world, to `apply`.
fn with_inventory<T>(
    world: &mut World,
    apply: impl FnOnce(&mut player_state::PlayerState, &mut UiRuntime, &World) -> Result<T, String>,
) -> Result<T, String> {
    world
        .try_resource_scope(|world, mut player: Mut<client_state::PlayerRuntime>| {
            world.try_resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
                apply(&mut player, &mut ui, world)
            })
        })
        .flatten()
        .unwrap_or_else(|| Err("the inventory is unavailable".into()))
}

/// Opens the personal inventory as the inventory key does; an open screen stays open.
fn open_screen(
    player: &mut player_state::PlayerState,
    ui: &mut UiRuntime,
    authority: Option<client_world::ui_read::InventoryUi<'_>>,
) -> Result<(), String> {
    if ui.inventory_open() {
        return Ok(());
    }
    ui.toggle_inventory(player, authority);
    if ui.inventory_open() {
        Ok(())
    } else {
        Err("the inventory cannot open yet".into())
    }
}

/// Closes the open screen as Escape does, returning held and crafting-grid items.
fn close_screen(player: &mut player_state::PlayerState, ui: &mut UiRuntime) -> Result<(), String> {
    if !ui.inventory_open() {
        return Err("no inventory screen is open".into());
    }
    ui.close_inventory(player);
    Ok(())
}

/// Runs the pointer action a plain, right or Shift click on the addressed cell produces.
fn click_cell(
    player: &mut player_state::PlayerState,
    ui: &mut UiRuntime,
    command: InventoryClick,
) -> Result<i32, String> {
    if !ui.inventory_open() {
        return Err("no inventory screen is open; send inventory_open first".into());
    }
    ui.observe_inventory_window(player);
    let screen = InventoryScreen::of_runtime(player, ui);
    let hit = cell_hit(ui.inventory_ledger(player), command.container, command.slot)
        .filter(|hit| {
            screens::screen_slots(screen)
                .iter()
                .any(|placed| placed.hit == *hit)
        })
        .ok_or_else(|| missing_cell(screen, command))?;
    let action = match (command.shift, command.button) {
        (true, _) => PointerAction::QuickMove(hit),
        (false, ClickButton::Left) => PointerAction::Click(hit),
        (false, ClickButton::Right) => PointerAction::SecondaryClick(hit),
    };
    ui.try_pointer_action(player, action)
        .map_err(|error| format!("the client refused the click: {error}"))
}

/// The cell `slot` of `container` addresses; whether the open screen shows it is checked
/// separately against the screen's own cell layout.
fn cell_hit(
    ledger: &PlayerInventoryLedger,
    container: InventoryContainer,
    slot: u16,
) -> Option<InventoryCellHit> {
    let small = u8::try_from(slot).ok();
    match container {
        InventoryContainer::Inventory => small.map(InventoryCellHit::Player),
        InventoryContainer::Armor => small.map(InventoryCellHit::Armor),
        InventoryContainer::Offhand => (slot == 0).then_some(InventoryCellHit::Offhand),
        InventoryContainer::Container => Some(InventoryCellHit::Storage(slot)),
        InventoryContainer::CraftingInput => ledger
            .crafting_grid()
            .slots()
            .nth(usize::from(slot))
            .map(InventoryCellHit::Craft),
        InventoryContainer::CraftingOutput => (slot == 0).then_some(InventoryCellHit::CraftOutput),
    }
}

/// The container a screen cell belongs to; widgets and catalog entries belong to none.
const fn container_of(hit: InventoryCellHit) -> Option<InventoryContainer> {
    Some(match hit {
        InventoryCellHit::Player(_) => InventoryContainer::Inventory,
        InventoryCellHit::Armor(_) => InventoryContainer::Armor,
        InventoryCellHit::Offhand => InventoryContainer::Offhand,
        InventoryCellHit::Storage(_) => InventoryContainer::Container,
        InventoryCellHit::Craft(_) => InventoryContainer::CraftingInput,
        InventoryCellHit::CraftOutput => InventoryContainer::CraftingOutput,
        InventoryCellHit::Widget(_)
        | InventoryCellHit::CreativeGrid(_)
        | InventoryCellHit::CreativeTab(_)
        | InventoryCellHit::CreativeSearch
        | InventoryCellHit::RecipeBook(_) => return None,
    })
}

/// Explains a click on a cell the open screen does not show, with the cells it does.
fn missing_cell(screen: InventoryScreen, command: InventoryClick) -> String {
    let available = screens::screen_slots(screen)
        .iter()
        .filter(|placed| container_of(placed.hit) == Some(command.container))
        .count();
    format!(
        "{} slot {} is not on the open {} screen, which has {available} such slots",
        wire_name(command.container),
        command.slot,
        screen_name(screen),
    )
}

/// The container's name as the protocol spells it.
fn wire_name(container: InventoryContainer) -> String {
    serde_json::to_value(container)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// A stable name for the drawn inventory screen.
const fn screen_name(screen: InventoryScreen) -> &'static str {
    match screen {
        InventoryScreen::Personal => "inventory",
        InventoryScreen::Workbench => "crafting_table",
        InventoryScreen::Storage(_) => "container",
        InventoryScreen::Window(..) => "window",
        InventoryScreen::Creative => "creative",
        InventoryScreen::Book => "book",
    }
}

/// The client's own view: open screen, cursor, player cells, crafting grid and any storage.
fn report(player: &player_state::PlayerState, ui: &UiRuntime) -> Value {
    let ledger = ui.inventory_ledger(player);
    let item = |stack: Option<&NetworkItemStack>| {
        stack
            .filter(|stack| stack.count > 0)
            .map_or(Value::Null, |stack| {
                json!({
                    "identifier": ledger
                        .negotiated_item_entry(stack.network_id)
                        .map(|entry| entry.identifier.as_ref()),
                    "count": stack.count,
                })
            })
    };
    let open = ui.inventory_open();
    let screen = open.then(|| InventoryScreen::of_runtime(player, ui));
    let grid = ledger.crafting_grid();
    let output = matches!(
        screen,
        Some(InventoryScreen::Personal | InventoryScreen::Workbench)
    )
    .then(|| ui_screens::inventory_capture::crafting_output(ledger, ui.crafting_match(player)))
    .flatten()
    .filter(|(_, ghost)| !ghost);
    let storage_cells = match screen {
        Some(InventoryScreen::Storage(cells) | InventoryScreen::Window(_, cells)) => {
            Some(cells.min(protocol::MAX_GENERIC_STORAGE_SLOTS))
        }
        _ => None,
    };
    json!({
        "open": open,
        "ready": open && ledger.screen_accepts_gestures(),
        "screen": screen.map(screen_name),
        "window": ledger.window_kind().map(|kind| format!("{kind:?}")),
        "pending_requests": ledger.pending_request_count(),
        "cursor": item(ledger.cursor_stack()),
        "inventory": (0..protocol::PLAYER_INVENTORY_SLOTS)
            .map(|slot| item(ledger.displayed_stack(slot)))
            .collect::<Vec<_>>(),
        "armor": (0..protocol::ARMOR_SLOTS)
            .map(|slot| item(ledger.target_stack(InventoryTarget::Armor(slot))))
            .collect::<Vec<_>>(),
        "offhand": item(ledger.target_stack(InventoryTarget::Offhand)),
        "crafting": {
            "width": grid.width(),
            "input": grid
                .slots()
                .map(|slot| item(ledger.target_stack(InventoryTarget::Craft(slot))))
                .collect::<Vec<_>>(),
            "output": item(output.as_ref().map(|(stack, _)| stack)),
        },
        "container": storage_cells.map(|cells| {
            (0..cells)
                .map(|slot| item(u16::try_from(slot).ok().and_then(|slot| ledger.storage_stack(slot))))
                .collect::<Vec<_>>()
        }),
    })
}

#[cfg(test)]
mod tests;
