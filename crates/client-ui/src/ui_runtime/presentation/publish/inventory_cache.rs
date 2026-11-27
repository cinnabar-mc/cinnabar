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
    /// The last observation of shield and trim art, which an unchanged observation repeats.
    pub(super) shield_art: Option<super::shield_art::ShieldArt>,
    /// Advances whenever item artwork sources change: a refreshed source or a repacked atlas.
    icon_epoch: u64,
    /// The last HUD capture an unchanged capture repeats.
    pub(super) hud_capture: Option<super::capture_reuse::CaptureRecord>,
}

/// The external sources projections read, retained so a replaced allocation never reuses an
/// address. [`SourceRefs`] is the same set borrowed from its owners.
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

/// This frame's sources, borrowed from their owners so an unchanged frame clones nothing.
struct SourceRefs<'a> {
    session: (u64, u64),
    registry: Option<&'a Arc<()>>,
    icons: Option<&'a Arc<assets::RuntimeIconCatalog>>,
    session_icons: u64,
    name_generation: u64,
    base_lang: Option<&'a Arc<assets::RuntimeLangCatalog>>,
    active_lang: Option<&'a Arc<assets::RuntimeLangCatalog>>,
    server_lang: Option<&'a Arc<assets::ServerLangOverlay>>,
    items: Option<&'a Arc<crate::ui_runtime::item_facts::SessionItemComponents>>,
    recipes: Option<&'a Arc<protocol::RecipeCatalog>>,
    crafting_revision: u64,
}

impl InventoryCache {
    /// Prepares revision-gated stack sources before any atlas placement or icon capture.
    /// Whether an external source changed, invalidating projections and closed-screen
    /// preparation.
    pub(super) fn prepare(
        &mut self,
        player: &player_state::PlayerState,
        items: &ItemResolver<'_>,
    ) -> bool {
        let runtime = items.runtime;
        let presentation = items.presentation;
        let current = SourceRefs {
            session: (runtime.session_id(), player.inventory.session_id()),
            registry: items
                .authority
                .map(|authority| authority.item_presentation_generation()),
            icons: presentation.icon_catalog.as_ref(),
            session_icons: presentation.session_icon_generation(),
            name_generation: runtime.item_name_generation(),
            base_lang: runtime.lang_catalog.as_ref(),
            active_lang: runtime.active_lang.as_ref(),
            server_lang: runtime.session.server_lang.as_ref(),
            items: runtime.session.session_items.as_ref(),
            recipes: player.inventory.crafting_catalog_allocation(),
            crafting_revision: player.inventory.crafting_revision(),
        };
        let refresh = current.refresh(self.sources.as_ref());
        // Every source takes part in a refresh, so an unrefreshed frame keeps the retained set.
        let refreshed = refresh != ProjectionRefresh::default();
        if refreshed || self.sources.is_none() {
            self.sources = Some(current.retain());
        }
        if refreshed {
            self.icon_epoch = self.icon_epoch.wrapping_add(1);
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
        refreshed
    }

    /// See [`InventoryProjectionCache::stacks_epoch`].
    pub(super) fn stacks_epoch(&self) -> u64 {
        self.projections.stacks_epoch()
    }

    /// Borrows artwork sources already produced by the inventory projection.
    pub(super) fn stacks(&self) -> impl Iterator<Item = &protocol::NetworkItemStack> {
        self.projections.stacks()
    }

    /// Drops cached artwork after a repack without repeating recipe matching.
    pub(in crate::ui_runtime::presentation) fn invalidate_icons(&mut self) {
        self.icon_epoch = self.icon_epoch.wrapping_add(1);
        self.projections.invalidate_icons();
    }

    /// Advances whenever item artwork sources change; see [`Self::invalidate_icons`].
    pub(super) fn icon_epoch(&self) -> u64 {
        self.icon_epoch
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

impl SourceRefs<'_> {
    /// Clones the borrowed allocations so they outlive this frame.
    fn retain(&self) -> Sources {
        Sources {
            session: self.session,
            registry: self.registry.cloned(),
            icons: self.icons.cloned(),
            session_icons: self.session_icons,
            name_generation: self.name_generation,
            base_lang: self.base_lang.cloned(),
            active_lang: self.active_lang.cloned(),
            server_lang: self.server_lang.cloned(),
            items: self.items.cloned(),
            recipes: self.recipes.cloned(),
            crafting_revision: self.crafting_revision,
        }
    }

    /// Separates external icon, durability, recipe and name invalidations since `previous`.
    fn refresh(&self, previous: Option<&Sources>) -> ProjectionRefresh {
        let Some(previous) = previous.filter(|previous| previous.session == self.session) else {
            return ProjectionRefresh {
                icons: true,
                durability: true,
                names: true,
                recipes: true,
            };
        };
        let registry = !same(self.registry, previous.registry.as_ref());
        ProjectionRefresh {
            icons: registry
                || self.session_icons != previous.session_icons
                || !same(self.icons, previous.icons.as_ref()),
            durability: registry,
            names: registry
                || self.name_generation != previous.name_generation
                || !same(self.base_lang, previous.base_lang.as_ref())
                || !same(self.active_lang, previous.active_lang.as_ref())
                || !same(self.server_lang, previous.server_lang.as_ref())
                || !same(self.items, previous.items.as_ref()),
            recipes: registry
                || !same(self.recipes, previous.recipes.as_ref())
                || self.crafting_revision != previous.crafting_revision,
        }
    }
}

/// Compares a current source allocation with a retained one by identity; the retained one keeps
/// its address from being reused, so a match is the same allocation.
fn same<T>(left: Option<&Arc<T>>, right: Option<&Arc<T>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
