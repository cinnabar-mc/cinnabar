//! Per-column scan results, rebuilt only when the column's blocks or block entities change.

use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

use assets::BlockEntityRouteKind;
use world::{BlockEntityKey, BlockEntityNbt, Chunk, SubChunk};

use super::{super::describe::Template, portals::PortalCell};

/// A column's portal cells and drawable block entities, valid while its maps are unchanged.
pub(super) struct ColumnScan {
    sub_chunks: Weak<BTreeMap<i32, Arc<SubChunk>>>,
    block_entities: Weak<BTreeMap<BlockEntityKey, Arc<BlockEntityNbt>>>,
    pub(super) portals: Vec<PortalCell>,
    /// Block entities routed to a model or text overlay, in key order.
    pub(super) entities: Vec<ColumnEntity>,
    /// Beam sections filled only after distance and submission admission.
    pub(super) beacons: BTreeMap<BlockEntityKey, render_model::BeaconModel>,
    /// The frame that last used this scan; older scans are dropped after each frame.
    pub(super) seen_frame: u64,
}

/// One routed block entity and the template its NBT and block describe.
pub(super) struct ColumnEntity {
    pub(super) key: BlockEntityKey,
    nbt: Arc<BlockEntityNbt>,
    runtime_id: u32,
    pub(super) template: Option<Template>,
}

impl ColumnScan {
    /// Records `chunk`'s current maps so a later edit to either invalidates this scan.
    pub(super) fn new(
        chunk: &Chunk,
        portals: Vec<PortalCell>,
        entities: Vec<ColumnEntity>,
    ) -> Self {
        Self {
            sub_chunks: Arc::downgrade(&chunk.shared_sub_chunks()),
            block_entities: Arc::downgrade(&chunk.shared_block_entities()),
            portals,
            entities,
            beacons: BTreeMap::new(),
            seen_frame: 0,
        }
    }

    /// Whether `chunk` still holds the maps this scan was built from.
    pub(super) fn is_current(&self, chunk: &Chunk) -> bool {
        chunk.holds_sub_chunks(&self.sub_chunks) && chunk.holds_block_entities(&self.block_entities)
    }
}

/// The terrain and column window one pass scanned: while they are unchanged after a pass
/// that left every column current, no column needs a rescan.
#[derive(Clone, PartialEq, Eq)]
pub(in crate::block_entities) struct ColumnWindow {
    pub(in crate::block_entities) terrain: u64,
    pub(in crate::block_entities) columns: [std::ops::RangeInclusive<i32>; 2],
    pub(in crate::block_entities) eye_column: [i32; 2],
}

impl ColumnWindow {
    /// Loaded columns of the window outside the eye's near reach whose scans `is_stale`
    /// reports out of date, in scan order.
    pub(in crate::block_entities) fn stale_distant<'a>(
        &'a self,
        dimension: i32,
        store: &'a world::ChunkStore,
        is_stale: impl Fn(world::ChunkKey, &Chunk) -> bool + 'a,
    ) -> impl Iterator<Item = world::ChunkKey> + 'a {
        let [xs, zs] = self.columns.clone();
        xs.flat_map(move |x| {
            zs.clone()
                .map(move |z| world::ChunkKey::new(dimension, x, z))
        })
        .filter(move |key| {
            !is_near_column(self.eye_column, key.x, key.z)
                && store.chunk(*key).is_some_and(|chunk| is_stale(*key, chunk))
        })
    }
}

/// Maximum distant-column rescans per frame; near edits bypass this streaming budget.
pub(in crate::block_entities) const MAX_COLUMN_RESCANS_PER_FRAME: usize = 8;

/// Rotates distant rescan admission without changing the scene's submission order.
#[derive(Default)]
pub(in crate::block_entities) struct RescanOrder {
    cursor: Option<world::ChunkKey>,
}

impl RescanOrder {
    /// Selects at most the frame limit from sorted dirty keys, wrapping after the last admission.
    pub(in crate::block_entities) fn select(
        &mut self,
        dirty: impl Iterator<Item = world::ChunkKey>,
    ) -> [Option<world::ChunkKey>; MAX_COLUMN_RESCANS_PER_FRAME] {
        let mut selected = [None; MAX_COLUMN_RESCANS_PER_FRAME];
        let mut wrapped = [None; MAX_COLUMN_RESCANS_PER_FRAME];
        let (mut next, mut first) = (0, 0);
        for key in dirty {
            if self.cursor.is_none_or(|cursor| key > cursor) {
                selected[next] = Some(key);
                next += 1;
                if next == selected.len() {
                    break;
                }
            } else if first < wrapped.len() {
                wrapped[first] = Some(key);
                first += 1;
            }
        }
        for key in wrapped.into_iter().take(first).take(selected.len() - next) {
            selected[next] = key;
            next += 1;
        }
        if next > 0 {
            self.cursor = selected[next - 1];
        }
        selected
    }
}
/// Columns within this many columns of the eye's rescan on every change outside the budget,
/// so blocks the player edits within reach never wait for streaming elsewhere.
const NEAR_COLUMN_REACH: i32 = 1;

/// Whether column (`chunk_x`, `chunk_z`) is one of the eye column's near neighbours.
pub(in crate::block_entities) fn is_near_column(
    eye_column: [i32; 2],
    chunk_x: i32,
    chunk_z: i32,
) -> bool {
    (chunk_x - eye_column[0]).abs() <= NEAR_COLUMN_REACH
        && (chunk_z - eye_column[1]).abs() <= NEAR_COLUMN_REACH
}

/// Reuses current scans and refreshes near or budgeted columns.
/// Deferred columns keep their stale scan, or return `None` until first scanned.
/// `is_current` checks the scan against the chunk without rebuilding it.
pub(in crate::block_entities) fn frame_scan<S>(
    previous: Option<S>,
    chunk: &Chunk,
    near: bool,
    rescans_left: &mut usize,
    is_current: impl Fn(&S, &Chunk) -> bool,
    rescan: impl FnOnce(Option<S>) -> S,
) -> Option<S> {
    match previous {
        Some(scan) if is_current(&scan, chunk) => Some(scan),
        previous if near => Some(rescan(previous)),
        previous if *rescans_left > 0 => {
            *rescans_left -= 1;
            Some(rescan(previous))
        }
        stale => stale,
    }
}

/// `chunk`'s block entities that route to a model or text overlay, or are an overlay host's
/// cell overlays, and sit on a loaded block.
/// An entity whose NBT and block are unchanged keeps its template from `previous`;
/// `describe` builds the rest from `(id, runtime id, NBT, block position)`.
pub(super) fn routed_entities(
    chunk: &Chunk,
    previous: Option<ColumnScan>,
    mut describe: impl FnMut(&str, u32, &BlockEntityNbt, [i32; 3]) -> Option<Template>,
) -> Vec<ColumnEntity> {
    let mut previous = previous.map_or_else(Vec::new, |scan| scan.entities);
    let sub_chunks = chunk.shared_sub_chunks();
    let mut entities = Vec::new();
    for (key, nbt) in chunk.shared_block_entities().iter() {
        let Some(id) = nbt.id() else {
            continue;
        };
        if nbt.cell_overlays().is_none()
            && !matches!(
                assets::block_entity_route(id),
                Some(BlockEntityRouteKind::Model | BlockEntityRouteKind::TextOverlay)
            )
        {
            continue;
        }
        let [x, y, z] = key.position();
        let Some(runtime_id) = sub_chunks.get(&key.sub_chunk().y).and_then(|sub_chunk| {
            sub_chunk.runtime_id(0, (x & 15) as u8, (y & 15) as u8, (z & 15) as u8)
        }) else {
            continue;
        };
        let unchanged = previous
            .binary_search_by(|entity| entity.key.cmp(key))
            .ok()
            .filter(|&index| {
                Arc::ptr_eq(&previous[index].nbt, nbt) && previous[index].runtime_id == runtime_id
            });
        let template = match unchanged {
            Some(index) => previous[index].template.take(),
            None => describe(id, runtime_id, nbt, [x, y, z]),
        };
        entities.push(ColumnEntity {
            key: *key,
            nbt: Arc::clone(nbt),
            runtime_id,
            template,
        });
    }
    entities
}

#[cfg(test)]
#[path = "columns_tests.rs"]
mod tests;
