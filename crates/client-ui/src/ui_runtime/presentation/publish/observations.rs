//! Runtime observations that must precede the HUD publication.

use crate::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

/// Derives the armor stat from all four worn stacks, retaining unknown protection.
pub(super) fn armor(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    authority: Option<client_world::ui_read::InventoryUi<'_>>,
) -> Option<u16> {
    let worn = runtime.local_armor(player_runtime);
    let worn = [&worn.helmet, &worn.chestplate, &worn.leggings, &worn.boots];
    let protection = worn.map(|stack| {
        if stack.is_empty() {
            Some(0)
        } else {
            authority.and_then(|authority| authority.item_armour(stack.network_id).protection())
        }
    });
    protection
        .into_iter()
        .try_fold(0u32, |sum, points| Some(sum.saturating_add(points?)))
        .map(|points| points.min(20) as u16)
}

/// Opens a lectern's first observed book before the active screen is selected.
pub(super) fn open_lectern(
    player_runtime: &player_state::PlayerState,
    runtime: &mut UiRuntime,
    world: Option<client_world::ui_read::InventoryUi<'_>>,
) {
    if runtime.inventory_open()
        && runtime.inventory_ledger(player_runtime).window_kind()
            == Some(protocol::WindowKind::Lectern)
        && runtime.screen_state().book.is_none()
        && let Some(position) = runtime.inventory_ledger(player_runtime).window_position()
        && let Some(nbt) = world.and_then(|world| world.block_entity_compound(position.into()))
    {
        let pages: Vec<String> = nbt
            .compound("book")
            .and_then(|book| book.compound("tag"))
            .and_then(|tag| tag.list("pages"))
            .map(|pages| {
                pages
                    .iter()
                    .map(|page| match page {
                        world::NbtValue::Compound(page) => {
                            page.string("text").unwrap_or_default().to_owned()
                        }
                        _ => String::new(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut state = crate::ui_runtime::book_screen::BookState::new(
            crate::ui_runtime::book_screen::BookSource::Lectern(position),
            pages,
            false,
            String::new(),
            String::new(),
        );
        state.page = nbt
            .integer("page")
            .and_then(|page| usize::try_from(page).ok())
            .map_or(0, |page| page.min(state.pages.len() - 1));
        runtime.open_book(state);
    }
}

/// Logs changed HUD counters once after the frame has been captured.
pub(super) fn note_diagnostics(runtime: &UiRuntime, presentation: &mut UiPresentationRuntime) {
    let diagnostics = runtime.gameplay_hud().diagnostics();
    if diagnostics != presentation.last_hud_diagnostics {
        tracing::debug!(
            skipped_effect_actions = diagnostics.skipped_effect_actions,
            evicted_effects = diagnostics.evicted_effects,
            odd_metadata_values = diagnostics.odd_metadata_values,
            dropped_inventory_events = diagnostics.dropped_inventory_events,
            unknown_container_events = diagnostics.unknown_container_events,
            odd_attribute_values = diagnostics.odd_attribute_values,
            odd_hud_packets = diagnostics.odd_hud_packets,
            oversized_chat_rows = diagnostics.oversized_chat_rows,
            unknown_effect_ids = diagnostics.unknown_effect_ids,
            "gameplay HUD skipped odd remote data"
        );
        presentation.last_hud_diagnostics = diagnostics;
    }
}
