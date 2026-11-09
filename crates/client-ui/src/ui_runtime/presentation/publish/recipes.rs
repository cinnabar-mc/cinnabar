//! Artwork for the open container and its recipe pages.

use super::item_icons::ItemResolver;
use crate::ui_runtime::presentation::{
    self,
    hud_layout::{WindowIcons, WindowText},
    inventory_pointer::InventoryScreen,
};
use std::borrow::Cow;

/// Captures the engine recipe list with its existing cache generation.
pub(super) fn recipe_entries(
    player_runtime: &player_state::PlayerState,
    items: &ItemResolver<'_>,
    inventory_screen: InventoryScreen,
    window_icons: &mut WindowIcons,
) {
    let runtime = items.runtime;
    let presentation = items.presentation;
    if matches!(
        inventory_screen,
        InventoryScreen::Personal
            | InventoryScreen::Workbench
            | InventoryScreen::Creative
            | InventoryScreen::Window(
                protocol::WindowKind::Furnace
                    | protocol::WindowKind::BlastFurnace
                    | protocol::WindowKind::Smoker,
                _
            )
    ) && presentation::forms::recipe_book_shown(player_runtime, runtime)
    {
        // Entries resolve icons like slots do, so enchanted and foil items glint here too.
        let icon = |stack: &protocol::NetworkItemStack| items.icon(stack, None);
        if ui_screens::furnace_book::active(player_runtime) {
            window_icons.furnace_entries = presentation::forms::furnace_book::icons(
                player_runtime,
                runtime,
                presentation.session_icon_generation(),
                icon,
            );
        } else {
            let sources = presentation::forms::BookIconSources {
                registry: items.authority.map(|authority| {
                    std::sync::Arc::clone(authority.item_presentation_generation())
                }),
                catalog: presentation.icon_catalog.clone(),
                session_icons: presentation.session_icon_generation(),
                dials: presentation.dial_frames(),
            };
            let icons =
                presentation::forms::recipe_book_icons(player_runtime, runtime, sources, icon);
            window_icons.book_entries.resize(icons.len(), None);
            window_icons.book_entries.copy_from_slice(&icons);
        }
    }
}

/// Captures the visible creative cells and tab artwork.
pub(super) fn creative_page(
    player_runtime: &player_state::PlayerState,
    items: &ItemResolver<'_>,
    inventory_screen: InventoryScreen,
    window_icons: &mut WindowIcons,
) {
    let runtime = items.runtime;
    let presentation = items.presentation;
    if inventory_screen == InventoryScreen::Creative {
        let entries =
            crate::ui_runtime::inventory_actions::visible_creative_entries(player_runtime, runtime);
        let first = runtime.screen_state().creative_row * presentation::screens::GRID_COLUMNS;
        for (cell, item) in entries
            .iter()
            .skip(first)
            .take(presentation::screens::GRID_CELLS)
            .enumerate()
        {
            window_icons.creative[cell] = items.icon(&item.stack, None);
        }
        for (tab, id) in [
            assets::facts::UiItemIcon::Construction.identifier(),
            assets::facts::UiItemIcon::Nature.identifier(),
            assets::facts::UiItemIcon::Equipment.identifier(),
            assets::facts::UiItemIcon::Items.identifier(),
            assets::facts::UiItemIcon::Search.identifier(),
        ]
        .iter()
        .enumerate()
        {
            window_icons.creative_tabs[tab] = presentation.item_icon(id, 0);
        }
    }
}

/// The stack sources shared by atlas preparation and container preview rendering.
#[derive(Default)]
pub(super) struct ScreenPreviews<'a> {
    choices: Option<Vec<Option<Cow<'a, protocol::NetworkItemStack>>>>,
    output: Option<Cow<'a, protocol::NetworkItemStack>>,
}

impl ScreenPreviews<'_> {
    /// Visits the same nonempty stacks that the container preview will draw.
    pub(super) fn stacks(&self) -> impl Iterator<Item = &protocol::NetworkItemStack> {
        self.choices
            .iter()
            .flat_map(|choices| choices.iter().flatten())
            .chain(self.output.iter())
            .map(|stack| stack.as_ref())
    }
}

/// Builds a plain stack for a station's declared recipe result.
fn output_stack(output: protocol::RecipeOutput) -> protocol::NetworkItemStack {
    protocol::NetworkItemStack {
        network_id: output.network_id,
        metadata: u32::from(output.aux),
        count: u16::from(output.count),
        block_runtime_id: i32::from_ne_bytes(output.block_runtime_id.to_ne_bytes()),
        ..protocol::NetworkItemStack::empty()
    }
}

/// Selects station and trading previews before any consumer retains their icon addresses.
pub(super) fn screen_previews<'a>(
    player: &'a player_state::PlayerState,
    runtime: &crate::ui_runtime::UiRuntime,
) -> ScreenPreviews<'a> {
    let ledger = runtime.inventory_ledger(player);
    let Some(kind) = ledger.window_kind() else {
        return ScreenPreviews::default();
    };
    if let Some(trade) = ledger.trading() {
        let choices = trade
            .offers
            .iter()
            .flat_map(|offer| {
                offer
                    .inputs
                    .iter()
                    .chain(std::iter::once(&offer.output))
                    .map(|stack| {
                        stack.as_ref().map(|stack| {
                            if stack.metadata == u32::from(protocol::RECIPE_ANY_AUX) {
                                let mut preview = stack.clone();
                                preview.metadata = 0;
                                Cow::Owned(preview)
                            } else {
                                Cow::Borrowed(stack)
                            }
                        })
                    })
            })
            .collect();
        return ScreenPreviews {
            choices: Some(choices),
            output: ledger.trade_output().map(Cow::Borrowed),
        };
    }
    let choices = (kind == protocol::WindowKind::Stonecutter).then(|| {
        let mut choices: Vec<_> = runtime
            .stonecutter_options(player)
            .iter()
            .take(ui_screens::container_actions::STONECUTTER_CELLS)
            .map(|recipe| recipe.output.map(|output| Cow::Owned(output_stack(output))))
            .collect();
        choices.resize(ui_screens::container_actions::STONECUTTER_CELLS, None);
        choices
    });
    let output = ledger
        .created_output_stack()
        .is_none()
        .then(|| {
            match kind {
                protocol::WindowKind::Anvil => runtime.anvil_output(player),
                protocol::WindowKind::Grindstone => runtime.grindstone_output(player),
                _ => None,
            }
            .or_else(|| runtime.predicted_screen_output(player).map(output_stack))
        })
        .flatten()
        .map(Cow::Owned);
    ScreenPreviews { choices, output }
}

/// Resolves artwork from the same station preview stacks used by atlas preparation.
pub(super) fn screen_outputs(
    player_runtime: &player_state::PlayerState,
    items: &ItemResolver<'_>,
    window_icons: &mut WindowIcons,
) {
    let previews = screen_previews(player_runtime, items.runtime);
    if let Some(choices) = previews.choices {
        window_icons.recipe = choices
            .iter()
            .map(|stack| stack.as_ref().and_then(|stack| items.icon(stack, None)))
            .collect();
    }
    if let Some(stack) = previews.output {
        window_icons.recipe_output = Some(ui_screens::container_source::StationOutput {
            icon: items.icon(&stack, None),
            durability: items.durability(&stack, None),
            stack: stack.into_owned(),
        });
    }
}

/// Captures the personal recipe page and its next-page indicator.
pub(super) fn book_page(
    player_runtime: &player_state::PlayerState,
    items: &ItemResolver<'_>,
    inventory_screen: InventoryScreen,
    window_text: &mut WindowText,
    window_icons: &mut WindowIcons,
) {
    let runtime = items.runtime;
    let presentation = items.presentation;
    if matches!(
        inventory_screen,
        InventoryScreen::Personal | InventoryScreen::Workbench
    ) {
        window_icons.book_button =
            presentation.item_icon(assets::facts::UiItemIcon::Book.identifier(), 0);
        window_text.book_title = runtime
            .translation("recipe.book.title")
            .map(|text| text.to_string());
        if crate::ui_runtime::presentation::forms::recipe_book_shown(player_runtime, runtime) {
            let first = runtime.screen_state().book_page * presentation::screens::BOOK_CELLS;
            let page =
                runtime.book_recipes(player_runtime, first, presentation::screens::BOOK_CELLS + 1);
            window_icons.book_more = page.len() > presentation::screens::BOOK_CELLS;
            for (cell, recipe) in page
                .iter()
                .take(presentation::screens::BOOK_CELLS)
                .enumerate()
            {
                let output = recipe.recipe.output();
                let stack = output_stack(output);
                window_icons.book[cell] = items.icon(&stack, None);
            }
        }
    }
}
