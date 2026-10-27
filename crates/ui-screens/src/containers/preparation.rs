//! Inputs that can arrive after the first closed-inventory preparation.

use std::sync::Arc;

use super::{ContainerInput, ContainerScreen, ScreenCache};
use crate::container_source::ContainerFrame;
use inventory::inventory_ledger::InventoryPresentationSnapshot;
use protocol::{CreativeContentEvent, PlayerGameMode};
use ui_engine::engine::{FormEngine, ScreenArt, layout_task::LayoutResources, screen::PaintFrame};

use ui_engine::engine::layout_task::LayoutTask;

use crate::container_source::ContainerSource;

pub(super) struct InventoryPreparation {
    catalog: Arc<json_ui::Catalog>,
    ledger: InventoryPresentationSnapshot,
    creative: Option<CreativeContentEvent>,
    game_mode: Option<PlayerGameMode>,
    controller: inventory::InventoryController,
    artwork: u64,
}

impl InventoryPreparation {
    /// Retains the sources a closed inventory prepared, without copying their contents.
    pub(super) fn capture(engine: &FormEngine, source: &dyn ContainerSource, artwork: u64) -> Self {
        let player = source.player();
        Self {
            catalog: Arc::clone(engine.catalog()),
            ledger: player.inventory.ledger().presentation_snapshot(),
            creative: player.inventory.ledger().creative_catalog().cloned(),
            game_mode: player.facts.player_game_mode(),
            controller: source.screen().inventory,
            artwork,
        }
    }

    /// Checks source identities before rebuilding any rows, bindings or measurements.
    pub(super) fn matches(
        &self,
        engine: &FormEngine,
        source: &dyn ContainerSource,
        artwork: u64,
    ) -> bool {
        let player = source.player();
        Arc::ptr_eq(&self.catalog, engine.catalog())
            && self.ledger.matches(player.inventory.ledger())
            && same_creative(
                self.creative.as_ref(),
                player.inventory.ledger().creative_catalog(),
            )
            && self.game_mode == player.facts.player_game_mode()
            && self.controller == source.screen().inventory
            && self.artwork == artwork
    }
}

/// Keeps both creative rows and their group definitions alive for identity comparisons.
fn same_creative(
    left: Option<&CreativeContentEvent>,
    right: Option<&CreativeContentEvent>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            Arc::ptr_eq(&left.items, &right.items) && Arc::ptr_eq(&left.groups, &right.groups)
        }
        (None, None) => true,
        _ => false,
    }
}

impl ContainerScreen {
    /// Requests later artwork without discarding the closed inventory's measurements.
    pub fn refresh_artwork(&mut self) {
        self.refresh_artwork = true;
    }

    /// Releases texture-dependent measurements and layout while retaining the bound screen.
    pub fn invalidate_textures(&mut self) {
        self.prewarmed = None;
        self.preparation_revision = self.preparation_revision.wrapping_add(1);
        if let Some(pending) = &mut self.preparing {
            pending.invalidate_measurements = true;
        }
        for cache in [&mut self.cache, &mut self.inventory].into_iter().flatten() {
            cache.invalidate_textures();
        }
    }

    /// Whether pending or ready initial preparation still needs its projected slot inputs.
    pub fn retains_prepared_inventory_inputs(&self) -> bool {
        self.preparing.is_some() || self.inventory.as_ref().is_some_and(|cache| !cache.live)
    }

    /// Whether a closed inventory still owns unfinished preparation work.
    #[cfg(any(test, feature = "test-support"))]
    pub fn inventory_preparation_pending(&self) -> bool {
        self.preparing.is_some()
    }

    /// Prepares layout and textures without starting live bindings or painting the closed screen.
    pub fn prepare(
        &mut self,
        engine: &FormEngine,
        source: &dyn ContainerSource,
        items: &ContainerFrame<'_>,
        input: ContainerInput<'_>,
        frame: &mut PaintFrame<'_>,
        resources: LayoutResources,
    ) {
        let prepared = match self.collect_preparation(engine, false) {
            std::task::Poll::Pending => return,
            std::task::Poll::Ready(Some(revision)) => revision == self.preparation_revision,
            std::task::Poll::Ready(None) => self.refresh_artwork,
        };
        if prepared {
            self.refresh_artwork = false;
            if let Some(render) = self
                .inventory
                .as_ref()
                .and_then(|cache| cache.laid.as_ref())
                .map(|(_, _, render)| render)
            {
                let _ = engine.prepare(ScreenArt::default(), frame, |_, _| Some(render.as_ref()));
            }
            return;
        }
        let _ = self.render(engine, source, items, input, frame, Some(resources));
    }

    /// Drops obsolete catalogs before servicing work; compatible jobs are polled or finished.
    pub(super) fn collect_preparation(
        &mut self,
        engine: &FormEngine,
        wait: bool,
    ) -> std::task::Poll<Option<u64>> {
        let Some(mut pending) = self.preparing.take() else {
            return std::task::Poll::Ready(None);
        };
        if !Arc::ptr_eq(&pending.catalog, engine.catalog()) {
            return std::task::Poll::Ready(None);
        }
        let result = if wait {
            std::task::Poll::Ready(pending.task.finish(engine))
        } else {
            pending.task.poll(engine)
        };
        let std::task::Poll::Ready(result) = result else {
            self.preparing = Some(pending);
            return std::task::Poll::Pending;
        };
        if let Some(mut prepared) = result {
            if pending.invalidate_measurements
                && let Some(cache) = &mut prepared.cache
            {
                cache.invalidate_textures();
            }
            self.install_prepared(prepared);
        }
        std::task::Poll::Ready(Some(pending.revision))
    }

    /// Publishes the worker-owned cache only after its complete initial layout.
    pub(super) fn install_prepared(&mut self, prepared: PreparedInventory) {
        self.resolves += usize::from(prepared.resolved);
        self.inventory = prepared.cache;
    }

    /// Whether the personal inventory needs preparation for these sources. An inventory already
    /// painted retains its live bindings and needs no prewarm when it closes.
    pub fn prewarm_inventory(
        &mut self,
        engine: &FormEngine,
        source: &dyn ContainerSource,
        artwork: u64,
    ) -> bool {
        if !self.needs_inventory_preparation(engine, source, artwork) {
            return self.preparing.is_some() || self.refresh_artwork;
        }
        self.refresh_artwork = false;
        self.preparation_revision = self.preparation_revision.wrapping_add(1);
        self.prewarmed = Some(InventoryPreparation::capture(engine, source, artwork));
        true
    }

    /// Checks whether closed-screen inputs need artwork and layout preparation.
    pub fn needs_inventory_preparation(
        &self,
        engine: &FormEngine,
        source: &dyn ContainerSource,
        artwork: u64,
    ) -> bool {
        !self
            .inventory
            .as_ref()
            .is_some_and(|cached| cached.live && Arc::ptr_eq(&cached.catalog, engine.catalog()))
            && !self
                .prewarmed
                .as_ref()
                .is_some_and(|prewarmed| prewarmed.matches(engine, source, artwork))
    }

    /// Moves the retained tree to one worker; closed-screen frames never bind or place controls.
    pub(super) fn start_preparing(
        &mut self,
        engine: &FormEngine,
        context: json_ui::Context,
        data: json_ui::DataSource,
        view: json_ui::ViewState,
        frame: &mut PaintFrame<'_>,
        resources: LayoutResources,
    ) {
        let mut cache = self.inventory.take();
        let (catalog, baseline) = (
            Arc::clone(engine.catalog()),
            Arc::clone(engine.base_catalog()),
        );
        let text = (frame.metrics.scale.get(), frame.language);
        // Browsers cannot start native threads. Keep their existing preparation path.
        #[cfg(target_arch = "wasm32")]
        {
            let _ = resources;
            let before = cache.as_ref().map(|cache| Arc::as_ptr(&cache.resolved));
            let _ = engine.prepare(ScreenArt::default(), frame, |env, root| {
                ScreenCache::render(
                    &mut cache,
                    &catalog,
                    &baseline,
                    super::ScreenLayout::Personal { book: false }.screen().0,
                    &context,
                    &data,
                    &view,
                    root,
                    env,
                    text,
                )
            });
            let resolved = cache.as_ref().map(|cache| Arc::as_ptr(&cache.resolved)) != before;
            self.install_prepared(PreparedInventory { cache, resolved });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let task_catalog = Arc::clone(&catalog);
            let task = LayoutTask::spawn(
                resources,
                frame.metrics,
                frame.content,
                engine.textures.metadata_revision(),
                move |env, root| {
                    let before = cache.as_ref().map(|cache| Arc::as_ptr(&cache.resolved));
                    let _ = ScreenCache::render(
                        &mut cache,
                        &catalog,
                        &baseline,
                        super::ScreenLayout::Personal { book: false }.screen().0,
                        &context,
                        &data,
                        &view,
                        root,
                        env,
                        text,
                    );
                    let resolved =
                        cache.as_ref().map(|cache| Arc::as_ptr(&cache.resolved)) != before;
                    PreparedInventory { cache, resolved }
                },
            );
            self.preparing = task.map(|task| PendingInventory {
                revision: self.preparation_revision,
                catalog: task_catalog,
                task,
                invalidate_measurements: false,
            });
        }
    }
}

/// The complete initial layout and whether it resolved a new screen definition.
pub(super) struct PreparedInventory {
    pub(super) cache: Option<ScreenCache>,
    resolved: bool,
}

/// A worker and the source identity that must still be compatible when it is collected.
pub(super) struct PendingInventory {
    revision: u64,
    catalog: Arc<json_ui::Catalog>,
    task: LayoutTask<PreparedInventory>,
    invalidate_measurements: bool,
}
