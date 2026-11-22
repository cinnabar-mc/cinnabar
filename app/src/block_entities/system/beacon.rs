//! Beacon beams: colour sections from the compiled tints of the blocks above each beacon.

use std::collections::BTreeMap;

use client_interaction::movement::PhysicsCollisionRegistries;
use render_model::{BeaconModel, BlockEntityKind};
use world::BlockEntityKey;

use super::{BlockEntityRuntime, block_info};

/// The runtime id stored at a world position, or `None` while its sub-chunk is not loaded.
fn runtime_id_at(store: &world::ChunkStore, dimension: i32, [x, y, z]: [i32; 3]) -> Option<u32> {
    store
        .sub_chunk(world::SubChunkKey::new(
            dimension,
            x.div_euclid(16),
            y.div_euclid(16),
            z.div_euclid(16),
        ))?
        .runtime_id(
            0,
            x.rem_euclid(16) as u8,
            y.rem_euclid(16) as u8,
            z.rem_euclid(16) as u8,
        )
}

/// Whether a complete first layer of base blocks sits under a beacon. Servers that never report
/// the beacon's `Levels` leave the client to decide from the loaded blocks. A beam needs only a
/// level of one, so higher layers, which may reach into unloaded columns, are not read; an
/// unloaded first-layer cell keeps the beam off until it can be read.
fn pyramid_is_built(
    store: &world::ChunkStore,
    dimension: i32,
    mode: assets::NetworkIdMode,
    [x, y, z]: [i32; 3],
) -> bool {
    let states = assets::block_presentation_state::pinned_block_presentation_states();
    (-1..=1).all(|dx| {
        (-1..=1).all(|dz| {
            runtime_id_at(store, dimension, [x + dx, y - 1, z + dz])
                .is_some_and(|id| states.get(mode, id).beacon_base)
        })
    })
}

/// The beam an admitted beacon draws this frame, or `None` when it is unlit or the dimension has
/// no height range. A server that reports `Levels` decides whether it is lit; otherwise the first
/// pyramid layer does. The beam's sections are built once per column scan in `cache`.
#[allow(clippy::too_many_arguments)]
pub(super) fn kind(
    cache: &mut BTreeMap<BlockEntityKey, BeaconModel>,
    key: BlockEntityKey,
    levels_reported: bool,
    top: Option<i32>,
    runtime: &mut BlockEntityRuntime,
    collisions: &PhysicsCollisionRegistries,
    assets: &assets::RuntimeAssets,
    store: &world::ChunkStore,
    dimension: i32,
    mode: assets::NetworkIdMode,
    position: [i32; 3],
) -> Option<BlockEntityKind> {
    let top =
        top.filter(|_| levels_reported || pyramid_is_built(store, dimension, mode, position))?;
    let beam = cache.entry(key).or_insert_with(|| {
        model(
            runtime, collisions, assets, store, dimension, mode, position, top,
        )
    });
    Some(BlockEntityKind::Beacon(beam.clone()))
}

/// Builds an admitted beacon's sections for caching in its owning column scan.
#[allow(clippy::too_many_arguments)]
fn model(
    runtime: &mut BlockEntityRuntime,
    collisions: &PhysicsCollisionRegistries,
    assets: &assets::RuntimeAssets,
    store: &world::ChunkStore,
    dimension: i32,
    mode: assets::NetworkIdMode,
    position: [i32; 3],
    top: i32,
) -> BeaconModel {
    let sections = assets::beacon::beacon_sections(position[1] + 1, top, |y| {
        let id = runtime_id_at(store, dimension, [position[0], y, position[2]]);
        let Some(id) = id else {
            return assets::beacon::BeaconMedium::Clear;
        };
        let tint = runtime.beacon_tints.get(mode, id);
        let info = block_info(runtime, collisions, mode, id);
        assets::beacon::beacon_medium(
            info.as_ref().map_or("", |info| &info.name),
            tint,
            assets.resolve(mode, id).light_properties().filter(),
        )
    });
    BeaconModel {
        sections: sections.into(),
    }
}
