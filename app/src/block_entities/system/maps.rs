//! Filled-map images: requesting the ones the player holds and turning them into canvases.

use bevy::prelude::{Res, ResMut};
use client_runtime::network::NetworkHandle;
use client_shell::ui_resources::UiRuntimeResource;

use super::BlockEntityRuntime;
use crate::runtime::{network::resource_packs::PackApplication, world::ClientWorld};

/// Requests each missing held or framed map once per session after output admission.
pub(super) fn request_missing_maps(
    mut runtime: ResMut<BlockEntityRuntime>,
    network: Option<Res<NetworkHandle<PackApplication>>>,
    mut world: ResMut<ClientWorld>,
    player: Res<client_state::PlayerRuntime>,
    ui: Option<Res<UiRuntimeResource>>,
) {
    let Some(network) = network else {
        runtime.missing_maps.clear();
        return;
    };
    let Some(stream) = world.stream.as_mut() else {
        runtime.missing_maps.clear();
        return;
    };
    let runtime = &mut *runtime;
    {
        let held = client_presentation::presentation::equipment::local_input(
            &player,
            stream,
            ui.as_deref().map(|ui| &ui.0),
            stream.local_player_runtime_id(),
        );
        for item in [held.main, held.off].into_iter().flatten() {
            if item.identifier.as_ref() == assets::facts::FILLED_MAP_IDENTIFIER
                && let Some(id) = item.map_id
                && stream.map_image(id).is_none()
            {
                runtime.missing_maps.push(id);
            }
        }
    }
    runtime.missing_maps.sort_unstable();
    runtime.missing_maps.dedup();
    for id in std::mem::take(&mut runtime.missing_maps) {
        if !stream.map_is_known(id)
            && network
                .send_inventory_packet(protocol::map_info_request_packet(id))
                .is_ok()
        {
            stream.admit_map_request(id);
        }
    }
}

/// Cache key for a map image at one revision.
pub(super) fn map_cache_key(map_id: i64, revision: u64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (map_id, revision).hash(&mut hasher);
    hasher.finish()
}

/// A 128x128 RGBA8 canvas from packed map pixels (red in the low byte).
pub(super) fn map_canvas(pixels: &[u32]) -> Vec<u8> {
    pixels
        .iter()
        .flat_map(|pixel| pixel.to_le_bytes())
        .collect()
}
