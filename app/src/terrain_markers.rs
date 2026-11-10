//! Shows the barrier, light-block and structure-void marker layers when vanilla does.
//!
//! Vanilla's level renderer camera shows a marker layer while a creative local player holds a
//! block of that layer: a barrier shows barriers, any light block shows every light block and a
//! structure void shows structure voids.

use assets::TerrainMarker;
use bevy::prelude::*;
use client_state::{ClientFrameSet, PlayerRuntime};
use protocol::PlayerGameMode;
use render_scene::TerrainMarkerLayers;

use crate::runtime::world::ClientWorld;

pub(crate) fn configure(app: &mut App) {
    app.init_resource::<TerrainMarkerLayers>()
        .add_systems(Update, publish.in_set(ClientFrameSet::WorldPublication));
}

fn publish(
    world: Option<Res<ClientWorld>>,
    player: Option<Res<PlayerRuntime>>,
    mut layers: ResMut<TerrainMarkerLayers>,
) {
    let shown = world
        .as_deref()
        .and_then(|world| world.stream.as_ref())
        .zip(player.as_deref())
        .map_or(0, |(stream, player)| {
            let held = player
                .selected_stack()
                .and_then(|stack| {
                    gameplay::block_use::held_block_store_id(
                        &client_interaction::movement::world_view::GameplayWorldView(stream),
                        stack.block_runtime_id,
                    )
                })
                .and_then(|id| {
                    stream
                        .runtime_assets()
                        .resolve_known(stream.network_id_mode(), id)
                })
                .and_then(|block| TerrainMarker::from_variant(block.kind(), block.variant()));
            shown_layers(player.facts.player_game_mode(), held)
        });
    if layers.0 != shown {
        layers.0 = shown;
    }
}

/// The marker layers shown for the local player's resolved `game_mode` while holding a block of
/// the `held` marker class.
fn shown_layers(game_mode: Option<PlayerGameMode>, held: Option<TerrainMarker>) -> u8 {
    if game_mode != Some(PlayerGameMode::Creative) {
        return 0;
    }
    held.map_or(0, TerrainMarker::layer)
}

#[cfg(test)]
#[path = "terrain_markers/tests.rs"]
mod tests;
