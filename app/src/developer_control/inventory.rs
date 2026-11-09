//! Inventory screen control: opens, clicks and closes the screen through the same UI runtime
//! calls the player's inventory key, pointer and Escape make, and reports the client's own
//! view of its items. Clicks are predicted and queued locally; nothing waits for the server.

use bevy::prelude::*;
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
use client_ui::ui_runtime::{
    UiRuntime,
    inventory_drag::PointerAction,
    presentation::{UiPresentationRuntime, inventory_pointer::InventoryScreen, screens},
    scene_stack::Scene,
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

/// Opens the player's inventory, replying once the screen accepts clicks. Like the inventory
/// key, it opens only from gameplay, never under another screen, mod panel or modal.
pub(super) fn open(world: &mut World, reply: Reply) {
    let opened = with_inventory(world, |player, ui, world| {
        let stream = world
            .get_resource::<ClientWorld>()
            .and_then(|world| world.stream.as_ref())
            .ok_or("the player is not in a world")?;
        if !ui.inventory_open()
            && (foreground(world, player, ui) != Scene::Gameplay || presentation_holds_input(world))
        {
            return Err(COVERED.into());
        }
        open_screen(player, ui, Some(stream.authority().inventory_ui()))
    });
    match opened {
        Ok(()) => super::state::wait(world, Condition::InventoryReady, Some(OPEN_WAIT_MS), reply),
        Err(error) => reply.send(Err(error)),
    }
}

/// Closes the open screen as Escape does and reports the client's view right after.
pub(super) fn close(world: &mut World) -> Result<Value, String> {
    with_inventory(world, |player, ui, world| {
        if ui.inventory_open() && !escape_reaches_inventory(world, player, ui) {
            return Err(COVERED.into());
        }
        close_screen(player, ui)
    })?;
    if let Some(mut focus) = world.get_resource_mut::<client_state::CursorFocus>() {
        focus.authorize_screen_return();
    }
    Ok(json!({ "closed": true, "inventory": snapshot(world) }))
}

/// Clicks one cell and reports the predicted result. A left press first moves a mod screen's
/// edit-box focus to the pressed point, as the pointer path does, so a focused overlay edit
/// box beside the inventory gives way to the click instead of refusing it.
pub(super) fn click(world: &mut World, command: InventoryClick) -> Result<Value, String> {
    let (hit, point, destroys) = with_inventory(world, |player, ui, world| {
        if ui.inventory_open() && !inventory_takes_pointer(world, player, ui) {
            return Err(COVERED.into());
        }
        resolve_cell(player, ui, presentation(world), command)
    })?;
    if command.button == ClickButton::Left {
        focus_mod_screens_at(world, point);
    }
    let request_id = with_inventory(world, |player, ui, _| {
        dispatch_cell(player, ui, hit, destroys, command)
    })?;
    Ok(json!({ "request_id": request_id, "inventory": snapshot(world) }))
}

/// Applies a left press's edit-box selection to the player mod's screens at a cell's GUI
/// point, or deselects them when the screen has no drawn point for it.
fn focus_mod_screens_at(world: &mut World, point: Option<[f32; 2]>) {
    let now = world
        .get_resource::<Time<Real>>()
        .map_or(0.0, Time::elapsed_secs_f64);
    let Some(mut view) = world.get_resource_mut::<UiPresentationResource>() else {
        return;
    };
    let logical = point.and_then(|gui| view.0.engine_container_logical_point(gui));
    view.0.mod_ui_mut().focus_mod_screen_pointer(logical, now);
}

/// The client's inventory view, or null before the UI runtime exists.
pub(super) fn snapshot(world: &World) -> Value {
    match (
        world.get_resource::<client_state::PlayerRuntime>(),
        world.get_resource::<UiRuntimeResource>(),
    ) {
        (Some(player), Some(ui)) => report(
            player,
            ui,
            presentation(world),
            inventory_takes_pointer(world, player, ui),
        ),
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
        Condition::InventoryReady if !inventory_takes_pointer(world, player, ui) => {
            Err(COVERED.into())
        }
        Condition::InventorySettled if ledger.pending_request_count() != 0 => Err(format!(
            "{} inventory requests are waiting for the server",
            ledger.pending_request_count()
        )),
        _ => Ok(report(
            player,
            ui,
            presentation(world),
            inventory_takes_pointer(world, player, ui),
        )),
    }
}

/// Whether the open inventory takes the player's pointer now: it is the scene on top, no mod
/// panel or client-part modal is up, and no player mod's view hides it. A focused mod edit box
/// does not block it; a press moves that focus first.
fn inventory_takes_pointer(
    world: &World,
    player: &player_state::PlayerState,
    ui: &UiRuntime,
) -> bool {
    foreground(world, player, ui) == Scene::Container
        && !presentation_holds_input(world)
        && !presentation(world).is_some_and(|view| view.mod_ui().mod_view_shown())
}

/// Whether Escape would close the open inventory now: it takes the pointer, and the checks
/// Escape itself uses pass, so a focused mod edit box keeps the screen open.
fn escape_reaches_inventory(
    world: &World,
    player: &player_state::PlayerState,
    ui: &UiRuntime,
) -> bool {
    inventory_takes_pointer(world, player, ui)
        && crate::ui_runtime::interaction::inventory::accepts_menu_cancel(ui, presentation(world))
}

/// Whether something outside the scene stack holds the player's input: a server experience's
/// consent prompt, a player mod's panel or a client-part modal, which the ordinary input
/// paths also check.
fn presentation_holds_input(world: &World) -> bool {
    world
        .get_resource::<client_state::ConsentInput>()
        .is_some_and(|consent| consent.0)
        || presentation(world).is_some_and(|view| {
            view.mod_ui().mod_panel_open() || view.mod_ui().experience_modal_open()
        })
}

/// Why a command refuses while another screen takes the player's input.
const COVERED: &str = "another screen is in front of the inventory";

/// The screen that takes the player's keyboard and pointer input now, as the input owner
/// decides it for the inventory's own input path.
fn foreground(world: &World, player: &player_state::PlayerState, ui: &UiRuntime) -> Scene {
    crate::ui_runtime::input_owner::top_scene(
        ui,
        player,
        world.get_resource::<client_shell::menu::MenuRuntime>(),
    )
}

/// The drawn UI, when the presentation runtime exists.
fn presentation(world: &World) -> Option<&UiPresentationRuntime> {
    world
        .get_resource::<UiPresentationResource>()
        .map(|view| &view.0)
}

/// The cells the open screen lets a pointer press, with a press point on an engine-drawn
/// screen. Without a presentation runtime (in tests) the fixed fallback layout stands in.
fn drawn_cells(
    screen: InventoryScreen,
    presentation: Option<&UiPresentationRuntime>,
) -> Vec<(InventoryCellHit, Option<[f32; 2]>)> {
    match presentation {
        Some(view) => view.inventory_screen_cells(screen),
        None => screens::screen_slots(screen)
            .iter()
            .map(|placed| (placed.hit, None))
            .collect(),
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

/// Closes the open screen as Escape does, returning held and crafting-grid items. An open
/// book editor saves its edits first and stays open when the save is refused.
fn close_screen(player: &mut player_state::PlayerState, ui: &mut UiRuntime) -> Result<(), String> {
    if !ui.inventory_open() {
        return Err("no inventory screen is open".into());
    }
    if !ui.commit_book(false) {
        return Err("the book could not be saved, so it stays open".into());
    }
    ui.close_inventory(player);
    Ok(())
}

/// Runs the pointer action a plain, right or Shift click on the addressed cell produces.
#[cfg(test)]
fn click_cell(
    player: &mut player_state::PlayerState,
    ui: &mut UiRuntime,
    presentation: Option<&UiPresentationRuntime>,
    command: InventoryClick,
) -> Result<i32, String> {
    let (hit, _, destroys) = resolve_cell(player, ui, presentation, command)?;
    dispatch_cell(player, ui, hit, destroys, command)
}

/// The cell a click addresses, its press point on an engine-drawn screen, and whether a
/// Shift-click there deletes the stack. The cell must be one the open screen lets a pointer
/// press, and a Shift-click takes the same layout-dependent route a pointer press does.
fn resolve_cell(
    player: &mut player_state::PlayerState,
    ui: &mut UiRuntime,
    presentation: Option<&UiPresentationRuntime>,
    command: InventoryClick,
) -> Result<(InventoryCellHit, Option<[f32; 2]>, bool), String> {
    if !ui.inventory_open() {
        return Err("no inventory screen is open; send inventory_open first".into());
    }
    ui.observe_inventory_window(player);
    let screen = InventoryScreen::of_runtime(player, ui);
    let target = cell_hit(ui.inventory_ledger(player), command.container, command.slot);
    let drawn = drawn_cells(screen, presentation);
    let (hit, point) = target
        .and_then(|hit| drawn.iter().find(|(cell, _)| *cell == hit).copied())
        .ok_or_else(|| missing_cell(screen, command, drawn.iter().map(|(cell, _)| *cell)))?;
    // A Shift-click on a creative-layout hotbar slot deletes its stack, as with the pointer.
    let destroys = command.shift
        && point.is_some_and(|point| {
            presentation.is_some_and(|view| view.inventory_auto_place_destroys(point))
        });
    Ok((hit, point, destroys))
}

/// Runs the pointer action of a resolved click on `hit`.
fn dispatch_cell(
    player: &mut player_state::PlayerState,
    ui: &mut UiRuntime,
    hit: InventoryCellHit,
    destroys: bool,
    command: InventoryClick,
) -> Result<i32, String> {
    let result = match (command.shift, command.button) {
        (true, _) if destroys => ui.destroy_on_auto_place(player, hit),
        (true, _) => ui.try_pointer_action(player, PointerAction::QuickMove(hit)),
        (false, ClickButton::Left) => ui.try_pointer_action(player, PointerAction::Click(hit)),
        (false, ClickButton::Right) => {
            ui.try_pointer_action(player, PointerAction::SecondaryClick(hit))
        }
    };
    result.map_err(|error| format!("the client refused the click: {error}"))
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
fn missing_cell(
    screen: InventoryScreen,
    command: InventoryClick,
    drawn: impl Iterator<Item = InventoryCellHit>,
) -> String {
    let available = drawn
        .filter(|hit| container_of(*hit) == Some(command.container))
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
fn report(
    player: &player_state::PlayerState,
    ui: &UiRuntime,
    presentation: Option<&UiPresentationRuntime>,
    foreground: bool,
) -> Value {
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
    // Report the crafting grid's output only where the screen draws that grid and its
    // output, including a creative player's personal crafting view. A station's result
    // (an anvil's, say) is not a crafting-grid output and is not reported here.
    let shows_output = screen.is_some_and(|screen| {
        let drawn = drawn_cells(screen, presentation);
        drawn
            .iter()
            .any(|(hit, _)| *hit == InventoryCellHit::CraftOutput)
            && drawn
                .iter()
                .any(|(hit, _)| matches!(hit, InventoryCellHit::Craft(_)))
    });
    let output = shows_output
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
        "ready": open && foreground && ledger.screen_accepts_gestures(),
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
