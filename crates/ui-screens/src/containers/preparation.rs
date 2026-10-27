//! Inputs that can arrive after the first closed-inventory preparation.

use std::sync::Arc;

use inventory::inventory_ledger::InventoryPresentationSnapshot;
use protocol::{CreativeContentEvent, PlayerGameMode};
use ui_engine::engine::FormEngine;

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
