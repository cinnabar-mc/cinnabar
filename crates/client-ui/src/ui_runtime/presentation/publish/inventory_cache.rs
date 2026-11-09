//! Connects inventory projection invalidation to the owning presentation sources.

use super::{ItemIconFrames, ItemResolver, item_icons::stack_icon};
use std::sync::Arc;
use ui_screens::inventory_capture::cache::{
    InventoryProjectionCache, ProjectedIcon, ProjectionAnimation, ProjectionInput,
    ProjectionRefresh, ProjectionUpdate,
};

#[derive(Default)]
pub(in crate::ui_runtime::presentation) struct InventoryCache {
    sources: Option<Sources>,
    projections: InventoryProjectionCache,
}

struct Sources {
    session: (u64, u64),
    registry: Option<Arc<()>>,
    icons: Option<Arc<assets::RuntimeIconCatalog>>,
    session_icons: u64,
    name_generation: u64,
    base_lang: Option<Arc<assets::RuntimeLangCatalog>>,
    active_lang: Option<Arc<assets::RuntimeLangCatalog>>,
    server_lang: Option<Arc<assets::ServerLangOverlay>>,
    items: Option<Arc<crate::ui_runtime::item_facts::SessionItemComponents>>,
    recipes: Option<Arc<protocol::RecipeCatalog>>,
    crafting_revision: u64,
}

impl InventoryCache {
    /// Prepares revision-gated stack sources before any atlas placement or icon capture.
    pub(super) fn prepare(&mut self, player: &player_state::PlayerState, items: &ItemResolver<'_>) {
        let runtime = items.runtime;
        let presentation = items.presentation;
        let next = Sources {
            session: (runtime.session_id(), player.inventory.session_id()),
            registry: items
                .authority
                .map(|authority| Arc::clone(authority.item_presentation_generation())),
            icons: presentation.icon_catalog.clone(),
            session_icons: presentation.session_icon_generation(),
            name_generation: runtime.item_name_generation(),
            base_lang: runtime.lang_catalog.clone(),
            active_lang: runtime.active_lang.clone(),
            server_lang: runtime.session.server_lang.clone(),
            items: runtime.session.session_items.clone(),
            recipes: player.inventory.crafting_catalog_snapshot(),
            crafting_revision: player.inventory.crafting_revision(),
        };
        let refresh = next.refresh(self.sources.as_ref());
        self.sources = Some(next);
        if refresh != ProjectionRefresh::default() {
            presentation
                .inventory_preparation_revision
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        self.projections.prepare(
            runtime.inventory_ledger(player),
            runtime.inventory_open() || presentation.inventory_inputs_needed(player, runtime),
            refresh,
            || runtime.crafting_match(player),
        );
    }

    /// Borrows artwork sources already produced by the inventory projection.
    pub(super) fn stacks(&self) -> impl Iterator<Item = &protocol::NetworkItemStack> {
        self.projections.stacks()
    }

    /// Drops cached artwork after a repack without repeating recipe matching.
    pub(in crate::ui_runtime::presentation) fn invalidate_icons(&mut self) {
        self.projections.invalidate_icons();
    }

    /// Resolves inventory display changes before screen and selected-item observations.
    pub(super) fn capture(
        &mut self,
        player: &player_state::PlayerState,
        items: &ItemResolver<'_>,
        frames: &ItemIconFrames,
    ) -> ProjectionUpdate {
        self.prepare(player, items);
        let runtime = items.runtime;
        let presentation = items.presentation;
        self.projections.capture(
            ProjectionInput {
                ledger: runtime.inventory_ledger(player),
                inventory_open: runtime.inventory_open()
                    || presentation.inventory_inputs_needed(player, runtime),
                refresh: ProjectionRefresh::default(),
                animation: ProjectionAnimation {
                    player: frames.0,
                    dials: presentation.dial_frames(),
                },
            },
            || runtime.crafting_match(player),
            |stack, animation| {
                let Some(item) = items.item(stack) else {
                    return ProjectedIcon::default();
                };
                let dial = item
                    .identifier
                    .as_deref()
                    .and_then(assets::icon::dial::dial_strip)
                    .map(|strip| strip.kind);
                ProjectedIcon {
                    dial,
                    icon: stack_icon(runtime, presentation, stack, item, animation),
                }
            },
            |stack, correction| items.durability(stack, correction),
            |stack| Some(runtime.localized_item_name(&items.identifier(stack)?)),
        )
    }
}

impl Sources {
    /// Separates external icon, durability, recipe and name invalidations.
    fn refresh(&self, previous: Option<&Self>) -> ProjectionRefresh {
        let Some(previous) = previous.filter(|previous| previous.session == self.session) else {
            return ProjectionRefresh {
                icons: true,
                durability: true,
                names: true,
                recipes: true,
            };
        };
        let registry = !same(&self.registry, &previous.registry);
        ProjectionRefresh {
            icons: registry
                || self.session_icons != previous.session_icons
                || !same(&self.icons, &previous.icons),
            durability: registry,
            names: registry
                || self.name_generation != previous.name_generation
                || !same(&self.base_lang, &previous.base_lang)
                || !same(&self.active_lang, &previous.active_lang)
                || !same(&self.server_lang, &previous.server_lang)
                || !same(&self.items, &previous.items),
            recipes: registry
                || !same(&self.recipes, &previous.recipes)
                || self.crafting_revision != previous.crafting_revision,
        }
    }
}

/// Compares retained source allocations without allowing address reuse to alias a cache key.
fn same<T>(left: &Option<Arc<T>>, right: &Option<Arc<T>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
