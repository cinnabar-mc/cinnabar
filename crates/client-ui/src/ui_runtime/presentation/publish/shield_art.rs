//! Keeps shield and trim icon art in step with the stacks the inventory and HUD show.
//!
//! Observing the stacks resolves every one through the session's item registry. While neither
//! the stacks nor anything that resolves them changed, the last observation is repeated instead:
//! the same trim icons are requested in the same order, so the trim cache's mark-and-sweep keeps
//! exactly what a fresh observation would, and the same shield appearances are prepared.

use super::{ItemResolver, UiPresentationRuntime, recipes, screens};
use inventory::inventory_ledger::InventoryTarget;
use protocol::NetworkItemStack;
use std::sync::Arc;

/// What an observation read besides the projected stacks, and what it requested.
pub(in crate::ui_runtime::presentation) struct ShieldArt {
    key: Key,
    requests: Requests,
}

/// Everything an observation reads that the projection's stack epoch and source refresh do not
/// already cover. Only a closed inventory is repeated: an open one also shows creative and recipe
/// stacks, so an observation made while it was open never repeats, even when the projected stacks
/// did not change across the close.
#[derive(PartialEq)]
struct Key {
    stacks: u64,
    open: bool,
    armor: [Option<NetworkItemStack>; 4],
    cursor: Option<NetworkItemStack>,
    offhand: Option<NetworkItemStack>,
}

/// The trim icons and shield appearances one observation requested, in request order.
#[derive(Debug, Default, PartialEq)]
struct Requests {
    trims: Vec<(Arc<str>, u32, render_api::ItemTrim, Option<u32>)>,
    shields: Vec<Arc<render_api::ShieldPattern>>,
}

/// Keeps the visible stack set stable before inventory caches retain any icon addresses.
/// Returns whether projections were invalidated or composed shield or trim art waits to be
/// packed, either of which changes what a frame draws.
pub(in crate::ui_runtime::presentation) fn prepare_shield_art(
    player: &player_state::PlayerState,
    runtime: &crate::ui_runtime::UiRuntime,
    presentation: &mut UiPresentationRuntime,
    world: Option<client_world::ui_read::InventoryUi<'_>>,
    resolve: impl Fn(&NetworkItemStack) -> Option<client_world::CanonicalItemStack>,
) -> bool {
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
    let key = Key {
        stacks: inventory_cache.stacks_epoch(),
        open: runtime.inventory_open(),
        armor: std::array::from_fn(|slot| {
            ledger
                .target_stack(InventoryTarget::Armor(slot as u8))
                .cloned()
        }),
        cursor: ledger.cursor_stack().cloned(),
        offhand: runtime.local_offhand_stack(player).cloned(),
    };
    let repeats = !refreshed
        && !runtime.inventory_open()
        && inventory_cache
            .shield_art
            .as_ref()
            .is_some_and(|last| last.key == key);
    let requests = if repeats {
        let last = inventory_cache.shield_art.take().expect("checked above");
        debug_assert_eq!(
            last.requests,
            observe(player, runtime, &inventory_cache, &key, &resolve),
            "a repeated shield and trim observation differs from a fresh one"
        );
        last.requests
    } else {
        observe(player, runtime, &inventory_cache, &key, &resolve)
    };
    presentation.gui_models.trim_icons.begin_frame();
    for (identifier, metadata, trim, color) in &requests.trims {
        presentation.trim_icon(identifier, *metadata, trim, *color);
    }
    presentation.prepare_shield_icons(requests.shields.iter().cloned());
    inventory_cache.shield_art = Some(ShieldArt { key, requests });
    presentation.inventory_cache = Some(inventory_cache);
    refreshed
        || presentation.gui_models.shield_icons.dirty()
        || presentation.gui_models.trim_icons.dirty()
}

/// Resolves the shown stacks into the trim icons and shield appearances they need: projected
/// stacks, an open inventory's creative and recipe stacks, worn armor, the cursor and the offhand.
fn observe(
    player: &player_state::PlayerState,
    runtime: &crate::ui_runtime::UiRuntime,
    inventory_cache: &super::inventory_cache::InventoryCache,
    key: &Key,
    resolve: &impl Fn(&NetworkItemStack) -> Option<client_world::CanonicalItemStack>,
) -> Requests {
    let mut requests = Requests::default();
    let mut observe = |stack: &NetworkItemStack| {
        let Some(item) = resolve(stack) else {
            return;
        };
        if let Some(pattern) = item.shield_pattern {
            requests.shields.push(pattern);
        }
        if let (Some(trim), Some(identifier)) = (item.trim, item.identifier) {
            let color = runtime.stack_facts(stack).custom_color;
            requests
                .trims
                .push((identifier, stack.metadata, trim, color));
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
    // Worn armor icons are resolved every frame, so their trims stay packed while the inventory
    // is closed.
    for stack in key.armor.iter().flatten() {
        observe(stack);
    }
    for stack in key.cursor.iter().chain(&key.offhand) {
        observe(stack);
    }
    requests
}

#[cfg(test)]
impl super::inventory_cache::InventoryCache {
    /// The trims the last shield and trim observation requested, in request order.
    pub(in crate::ui_runtime::presentation) fn requested_trims(&self) -> Vec<render_api::ItemTrim> {
        self.shield_art
            .iter()
            .flat_map(|art| art.requests.trims.iter())
            .map(|(_, _, trim, _)| trim.clone())
            .collect()
    }
}
