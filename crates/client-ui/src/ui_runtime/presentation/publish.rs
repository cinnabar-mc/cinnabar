use {super::*, ui::IconRef};

pub mod commit;
pub mod item_icons;
pub mod reuse;
use item_icons::{ItemIconFrames, ItemResolver};

mod container;
pub(super) mod inventory_cache;
mod observations;
mod player_items;
mod recipes;

/// What one HUD capture produced besides the frame values it stored.
pub struct HudCapture {
    /// The held and offhand item icons the first-person viewmodel draws.
    pub icons: (Option<IconRef>, Option<IconRef>),
    /// Whether the capture changed anything a frame build reads of the UI runtime.
    pub runtime_changed: bool,
    /// Whether it changed anything a frame build reads of the presentation.
    pub presentation_changed: bool,
}

/// Replaces `slot` with `value`; whether it differed.
fn store<T: PartialEq>(slot: &mut T, value: T) -> bool {
    if *slot == value {
        return false;
    }
    *slot = value;
    true
}

/// Captures HUD display values and applies its required local authority observations, storing
/// only values that changed so an idle capture changes nothing. `first_person` is whether the HUD
/// draws for a first-person view with the hand shown.
pub fn capture_hud_frame(
    player_runtime: &player_state::PlayerState,
    runtime: &mut UiRuntime,
    presentation: &mut UiPresentationRuntime,
    world: Option<client_world::ui_read::InventoryUi<'_>>,
    first_person: bool,
    now_millis: u64,
    icon_frames: ItemIconFrames,
) -> HudCapture {
    let mut presentation_changed =
        dynamic_textures::observe_session(presentation, runtime.session_id());
    presentation_changed |= presentation.capture_local_player_identity(
        world.map(|world| world.local_player()),
        runtime.session_id(),
    );
    let mut runtime_changed = runtime.sync_anvil_name(player_runtime);
    runtime_changed |= observe_storage_block(runtime, player_runtime, world);
    // Inventory capture precedes lectern opening and screen selection. Selected-item
    // and sleep observations run after item capture, immediately before publication.
    let armor = observations::armor(player_runtime, runtime, world);
    runtime_changed |= runtime.set_derived_armor(armor);
    let mount_health = runtime
        .gameplay_hud()
        .mount_unique_id()
        .and_then(|unique| world.and_then(|world| world.actor_health_by_unique(unique)));
    presentation_changed |=
        prepare_shield_art(player_runtime, runtime, presentation, world, |stack| {
            world?.canonical_item_stack(stack)
        });
    let mut inventory_cache = presentation
        .inventory_cache
        .take()
        .expect("inventory cache retained");
    let items = ItemResolver {
        runtime,
        presentation,
        authority: world,
    };
    let projections = inventory_cache.capture(player_runtime, &items, &icon_frames);
    presentation.inventory_cache = Some(inventory_cache);
    let mut window_icons = container::ghost_icons(presentation, runtime.inventory_open());
    window_icons.ui = projections
        .decorations
        .as_ref()
        .map_or(presentation.hud_frame.window_icons.ui, |(_, icons)| *icons);
    runtime_changed |= observations::open_lectern(player_runtime, runtime, world);
    let items = ItemResolver {
        runtime,
        presentation,
        authority: world,
    };
    let (window_text, book_changed) =
        container::capture(player_runtime, &items, world, &mut window_icons);
    let cursor_icon = runtime
        .inventory_ledger(player_runtime)
        .cursor_stack()
        .and_then(|stack| items.icon(stack, None));
    let ledger = runtime.inventory_ledger(player_runtime);
    let cursor_durability = ledger.cursor_stack().and_then(|stack| {
        items.durability(
            stack,
            ledger
                .presented_cursor_overlay()
                .and_then(|overlay| overlay.durability_correction),
        )
    });
    let selected = player_items::SelectedItem::capture(player_runtime);
    let hotbar = player_items::hotbar(
        runtime.inventory_ledger(player_runtime),
        &selected,
        &items,
        &icon_frames,
    );
    // Logging only; no frame reads it.
    presentation.note_hotbar(hotbar.logged);
    let equipment = player_items::equipment(
        player_runtime,
        &selected,
        &ItemResolver {
            runtime,
            presentation,
            authority: world,
        },
        &icon_frames,
    );
    let mount_jump = runtime.gameplay_hud().mount_unique_id().and_then(|unique| {
        world
            .filter(|world| world.mount_has_jump(unique))
            .map(|_| runtime.mount_jump_charge(now_millis))
    });
    runtime_changed |=
        runtime.observe_selected_item_identity_value(selected.identity(), now_millis);
    let sleeping = world
        .and_then(|world| world.local_player().actor())
        .is_some_and(|actor| actor.is_sleeping());
    runtime_changed |= runtime.set_local_sleeping(sleeping);
    let frame = presentation.hud_frame_mut();
    let mut changed = frame.sleep.observe(sleeping, now_millis);
    changed |= store(&mut frame.first_person, first_person);
    changed |= store(&mut frame.mount_health, mount_health);
    changed |= store(&mut frame.hotbar_durability, hotbar.durability);
    changed |= store(&mut frame.hotbar_stacks, hotbar.stacks);
    changed |= store(&mut frame.offhand_durability, equipment.offhand_durability);
    changed |= store(&mut frame.hotbar_icons, hotbar.icons);
    if let Some(contents) = projections.contents {
        changed |= store(&mut frame.inventory_icons, contents.inventory_icons);
        changed |= store(&mut frame.storage_icons, contents.storage_icons);
        changed |= store(&mut frame.crafting, contents.crafting);
    }
    if let Some(names) = projections.item_names {
        changed |= store(&mut frame.item_names, names);
    }
    // The capture borrowed the retained recipe-book entries, so they return either way.
    if book_changed || window_icons.differs_besides_book(&frame.window_icons) {
        changed = true;
    }
    frame.window_icons = window_icons;
    if let Some((durability, _)) = projections.decorations {
        changed |= store(&mut frame.durability, durability);
    }
    changed |= store(&mut frame.window_text, window_text);
    changed |= store(&mut frame.cursor_icon, cursor_icon);
    changed |= store(&mut frame.cursor_durability, cursor_durability);
    changed |= store(&mut frame.armor_icons, equipment.armor_icons);
    changed |= store(&mut frame.armor_durability, equipment.armor_durability);
    changed |= store(&mut frame.offhand_icon, equipment.offhand_icon);
    // The preview, hand and viewmodel carriers are publication's: it sets them before every
    // build from the preview and the returned icons.
    changed |= store(
        &mut frame.viewmodel_pitch_degrees,
        world
            .and_then(|world| world.local_player().actor())
            .map_or(0.0, |actor| actor.pitch),
    );
    changed |= store(&mut frame.selected_item_name, equipment.selected_item_name);
    changed |= store(&mut frame.holding_filled_map, equipment.holding_filled_map);
    changed |= store(&mut frame.mount_jump, mount_jump);
    changed |= store(&mut frame.attack_indicator_charge, Some(1.0));
    // Logging only; no frame reads it.
    observations::note_diagnostics(runtime, presentation);
    HudCapture {
        icons: (equipment.held_item_icon, equipment.offhand_icon),
        runtime_changed,
        presentation_changed: presentation_changed | changed,
    }
}

/// Keeps the visible stack set stable before inventory caches retain any icon addresses.
/// Returns whether projections were invalidated or composed shield or trim art waits to be
/// packed, either of which changes what a frame draws.
pub(super) fn prepare_shield_art(
    player: &player_state::PlayerState,
    runtime: &UiRuntime,
    presentation: &mut UiPresentationRuntime,
    world: Option<client_world::ui_read::InventoryUi<'_>>,
    resolve: impl Fn(&protocol::NetworkItemStack) -> Option<client_world::CanonicalItemStack>,
) -> bool {
    use inventory::inventory_ledger::InventoryTarget;
    let mut inventory_cache = presentation
        .inventory_cache
        .take()
        .expect("inventory cache retained");
    let refreshed = inventory_cache.prepare(
        player,
        &ItemResolver {
            runtime,
            presentation,
            authority: world,
        },
    );
    let ledger = runtime.inventory_ledger(player);
    let mut appearances = Vec::new();
    presentation.gui_models.trim_icons.begin_frame();
    let mut observe = |stack: &protocol::NetworkItemStack| {
        if let Some(item) = resolve(stack) {
            if let Some(pattern) = item.shield_pattern {
                appearances.push(pattern);
            }
            if let (Some(trim), Some(identifier)) = (item.trim, item.identifier) {
                presentation.trim_icon(
                    &identifier,
                    stack.metadata,
                    &trim,
                    runtime.stack_facts(stack).custom_color,
                );
            }
        }
    };
    for stack in inventory_cache.stacks() {
        observe(stack);
    }
    if runtime.inventory_open() {
        let creative =
            crate::ui_runtime::inventory_actions::visible_creative_entries(player, runtime);
        for item in creative
            .iter()
            .skip(runtime.screen_state().creative_row * screens::GRID_COLUMNS)
            .take(screens::GRID_CELLS)
        {
            observe(&item.stack);
        }
        for stack in recipes::screen_previews(player, runtime).stacks() {
            observe(stack);
        }
    }
    // Worn armor icons are resolved every frame, so their trims stay packed while the inventory is closed.
    for stack in (0..4).filter_map(|slot| ledger.target_stack(InventoryTarget::Armor(slot))) {
        observe(stack);
    }
    for stack in ledger
        .cursor_stack()
        .into_iter()
        .chain(runtime.local_offhand_stack(player))
    {
        observe(stack);
    }
    presentation.inventory_cache = Some(inventory_cache);
    presentation.prepare_shield_icons(appearances);
    refreshed
        || presentation.gui_models.shield_icons.dirty()
        || presentation.gui_models.trim_icons.dirty()
}

/// The hovered container cell's stack and its custom name: what the tooltip shows, and what a
/// player mod's key reports as the slot under the pointer (JEI's `getSlotUnderMouse`).
pub fn hovered_stack(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
) -> Option<(protocol::NetworkItemStack, Option<std::sync::Arc<str>>)> {
    runtime.screen_state().hover.and_then(|hit| {
        use ui_screens::container_actions::InventoryCellHit as Hit;
        let ledger = runtime.inventory_ledger(player_runtime);
        let (stack, name) = match hit {
            Hit::Player(slot) => (
                ledger.displayed_stack(slot),
                ledger
                    .presented_slot_overlay(slot)
                    .and_then(|overlay| overlay.custom_name.clone()),
            ),
            Hit::Storage(slot) => (ledger.storage_stack(slot), None),
            Hit::Craft(slot) => (
                ledger
                    .target_stack(inventory::inventory_ledger::InventoryTarget::Craft(slot))
                    .or_else(|| ledger.crafting_ghost_stack(slot)),
                None,
            ),
            Hit::Armor(row) => (
                ledger.target_stack(inventory::inventory_ledger::InventoryTarget::Armor(row)),
                None,
            ),
            Hit::Offhand => (runtime.local_offhand_stack(player_runtime), None),
            Hit::CraftOutput
                if matches!(
                    ledger.window_kind(),
                    None | Some(protocol::WindowKind::Workbench)
                ) =>
            {
                return ui_screens::inventory_capture::crafting_output(
                    ledger,
                    player_runtime.inventory.crafting_match(),
                )
                .map(|(stack, _)| (stack, None));
            }
            Hit::CraftOutput if ledger.window_kind() == Some(protocol::WindowKind::Anvil) => {
                return runtime
                    .anvil_output(player_runtime)
                    .map(|stack| (stack, None));
            }
            Hit::CraftOutput if ledger.window_kind() == Some(protocol::WindowKind::Grindstone) => {
                return runtime
                    .grindstone_output(player_runtime)
                    .map(|stack| (stack, None));
            }
            Hit::CraftOutput
                if matches!(
                    ledger.window_kind(),
                    Some(protocol::WindowKind::Trade { .. })
                ) =>
            {
                (ledger.trade_output(), None)
            }
            Hit::CraftOutput => (ledger.created_output_stack(), None),
            Hit::CreativeGrid(index) => {
                let position = runtime.screen_state().creative_row * super::screens::GRID_COLUMNS
                    + usize::from(index);
                let entries = crate::ui_runtime::inventory_actions::visible_creative_entries(
                    player_runtime,
                    runtime,
                );
                (entries.get(position).map(|item| &item.stack), None)
            }
            Hit::Widget(ui_screens::container_actions::Widget::BookRecipe(index)) => {
                let skip = runtime.screen_state().book_page * super::screens::BOOK_CELLS
                    + usize::from(index);
                let output = runtime
                    .book_recipes(player_runtime, skip, 1)
                    .first()
                    .map(|entry| entry.recipe.output());
                return output.map(|output| {
                    let stack = protocol::NetworkItemStack {
                        network_id: output.network_id,
                        metadata: u32::from(output.aux),
                        count: u16::from(output.count),
                        block_runtime_id: i32::from_ne_bytes(output.block_runtime_id.to_ne_bytes()),
                        ..protocol::NetworkItemStack::empty()
                    };
                    (stack, None)
                });
            }
            Hit::Widget(ui_screens::container_actions::Widget::TradeOffer {
                tier,
                row,
                item: Some(column),
            }) => {
                let trade = ledger.trading()?;
                (trade.preview(trade.row_index(tier, row)?, column), None)
            }
            Hit::RecipeBook(index) => {
                return super::forms::recipe_book_hover(player_runtime, runtime, index);
            }
            Hit::Widget(_) | Hit::CreativeTab(_) | Hit::CreativeSearch => (None, None),
        };
        stack.map(|stack| (stack.clone(), name))
    })
}

/// Retains the block entity id under the open window, allocating only when it changes; whether
/// it changed.
fn observe_storage_block(
    runtime: &mut UiRuntime,
    player_runtime: &player_state::PlayerState,
    world: Option<client_world::ui_read::InventoryUi<'_>>,
) -> bool {
    let position = runtime.inventory_ledger(player_runtime).window_position();
    let block = position
        .zip(world)
        .and_then(|(position, world)| world.block_entity_compound(position.into()));
    let (Some(position), Some(id)) = (position, block.as_deref().and_then(|nbt| nbt.string("id")))
    else {
        return runtime.session.storage_block.take().is_some();
    };
    if runtime
        .session
        .storage_block
        .as_ref()
        .is_none_or(|(retained, old)| *retained != position || **old != *id)
    {
        runtime.session.storage_block = Some((position, std::sync::Arc::from(id)));
        return true;
    }
    false
}

#[cfg(test)]
mod world_read_tests;
