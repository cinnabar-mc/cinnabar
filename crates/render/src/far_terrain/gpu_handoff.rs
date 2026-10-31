//! Retains covering GPU allocations through ordered presentation changes.
use super::*;
use far_terrain::{
    HANDOFF_DEPTH_SHIFT, HANDOFF_DIRECTION_SHIFT, HANDOFF_FAR, HANDOFF_NEAR, HANDOFF_STEPS, Handoff,
};

// `far_terrain.wesl` ranks samples in a four-by-four ordered pattern.
const _: () = assert!(HANDOFF_STEPS == 16);

impl FarTerrainGpu {
    /// Completes proven replacements before a new selection changes their coverage evidence.
    pub(super) fn finish_tile_handoffs(&mut self) {
        for key in self.tile_handoffs.keys() {
            if let Some(old) = self.resident.remove(key) {
                self.ranges.release(old.quads);
                self.free_slots.push(old.slot);
            }
        }
        self.tile_handoffs.clear();
        self.presentation_steps.clear();
    }

    /// Starts whole overlapping groups together, keeping every blocked group's cover intact.
    /// Without a presentation clock a covered predecessor retires at once.
    pub(super) fn update_coverage(
        &mut self,
        tiles: &[Arc<TileMesh>],
        wanted: &HashMap<TileKey, &Arc<TileMesh>>,
        now: Option<Duration>,
    ) {
        let mut fallbacks: Vec<_> = self
            .resident
            .keys()
            .copied()
            .filter(|key| !wanted.contains_key(key))
            .collect();
        let ready: HashSet<_> = fallbacks
            .iter()
            .copied()
            .filter(|key| self.replaced(*key, tiles, None))
            .collect();
        let blocked: Vec<_> = wanted
            .keys()
            .copied()
            .filter(|key| {
                fallbacks
                    .iter()
                    .any(|old| old.overlaps(*key) && !ready.contains(old))
            })
            .collect();
        for key in ready {
            if blocked.iter().any(|selected| selected.overlaps(key)) {
                continue;
            }
            match now {
                Some(now) => {
                    self.tile_handoffs
                        .entry(key)
                        .or_insert_with(|| Handoff::new(now, false));
                }
                None => {
                    let old = self.resident.remove(&key).expect("covered fallback");
                    self.ranges.release(old.quads);
                    self.free_slots.push(old.slot);
                }
            }
        }
        fallbacks.retain(|key| self.resident.contains_key(key));
        self.draws.clear();
        for resident in self.resident.values_mut() {
            let key = resident.mesh.key;
            resident.visible = !wanted.contains_key(&key)
                || !fallbacks
                    .iter()
                    .any(|old| old.overlaps(key) && !self.tile_handoffs.contains_key(old));
        }
        // Keep scene order stable for view queues and append only retained predecessors.
        for key in tiles.iter().map(|mesh| mesh.key).chain(fallbacks) {
            let Some(resident) = self.resident.get(&key).filter(|resident| resident.visible) else {
                continue;
            };
            if !resident.mesh.quads.is_empty() {
                self.draws.push(FarDraw {
                    slot: resident.slot,
                    quads: resident.quads.clone(),
                    bounds: tile_bounds(&resident.mesh),
                });
            }
        }
    }

    /// Advances only active transitions; unchanged quantized samples issue no texture writes.
    pub(super) fn advance_handoffs(&mut self, queue: &RenderQueue, now: Duration) -> bool {
        let steps: HashMap<_, _> = self
            .tile_handoffs
            .iter()
            .map(|(&key, fade)| (key, fade.step(now)))
            .collect();
        if steps == self.presentation_steps {
            return false;
        }
        self.presentation_steps = steps;
        let expired: Vec<_> = self
            .tile_handoffs
            .iter()
            .filter(|(_, fade)| fade.complete(now))
            .map(|(&key, _)| key)
            .collect();
        let changed = !expired.is_empty();
        for key in expired {
            self.tile_handoffs.remove(&key);
            if let Some(old) = self.resident.remove(&key) {
                self.draws.retain(|draw| draw.slot != old.slot);
                self.ranges.release(old.quads);
                self.free_slots.push(old.slot);
            }
        }
        if changed {
            self.publication_revision = self.publication_revision.wrapping_add(1);
        }
        self.refresh_presentation(queue, now);
        changed
    }

    /// Publishes the same near/far transition that keeps near allocations admitted.
    pub(crate) fn publish_handoffs(
        &mut self,
        queue: &RenderQueue,
        columns: impl Iterator<Item = (ColumnPos, u32)>,
        now: Duration,
    ) {
        let columns = columns.collect();
        if self.column_handoffs != columns {
            self.column_handoffs = columns;
            self.refresh_presentation(queue, now);
        }
    }

    /// Updates colour alpha only when a tile's per-column presentation actually changes.
    pub(super) fn refresh_presentation(&mut self, queue: &RenderQueue, now: Duration) {
        let fades: HashMap<_, _> = self
            .tile_handoffs
            .iter()
            .flat_map(|(&key, fade)| {
                let step = fade.step(now);
                key.columns().map(move |column| (column, (key, step)))
            })
            .collect();
        let ready: HashSet<_> = if self.empty_far.is_empty() {
            HashSet::new()
        } else {
            self.resident
                .values()
                .filter(|resident| self.tile_goal.contains(&resident.mesh.key))
                .flat_map(|resident| resident.mesh.covered_columns())
                .collect()
        };
        let touched: Vec<ColumnPos> = fades
            .keys()
            .chain(self.column_handoffs.keys())
            .copied()
            .collect();
        let steady = HANDOFF_STEPS as u8;
        for resident in self
            .resident
            .values_mut()
            .filter(|resident| resident.visible)
        {
            let key = resident.mesh.key;
            // Untouched steady tiles keep their uploaded codes; only transitions pay per step.
            if resident.presentation.iter().all(|&code| code == steady)
                && !touched
                    .iter()
                    .any(|column| super::coverage::contains_column(key, *column))
            {
                continue;
            }
            let codes: Vec<_> = key
                .columns()
                .map(|column| {
                    let mut opacity = HANDOFF_STEPS;
                    let mut rank = 0;
                    if let Some(&(old, step)) = fades.get(&column) {
                        if old == key {
                            if self.empty_far.contains(&column) && !ready.contains(&column) {
                                opacity = HANDOFF_STEPS - step;
                            }
                        } else if self.tile_goal.contains(&key) {
                            opacity = step;
                            rank = 1;
                        }
                    }
                    if let Some(&code) = self.column_handoffs.get(&column) {
                        let step = code & 255;
                        match code >> HANDOFF_DIRECTION_SHIFT {
                            HANDOFF_NEAR => opacity = opacity.min(step),
                            HANDOFF_FAR => opacity = opacity.min(HANDOFF_STEPS - step),
                            _ => {}
                        }
                        rank += 2;
                    }
                    (opacity | rank << HANDOFF_DEPTH_SHIFT) as u8
                })
                .collect();
            if resident.presentation == codes {
                continue;
            }
            resident.presentation = codes;
            queue.profiled_write_texture(
                TexelCopyTextureInfo {
                    texture: &self.colours,
                    mip_level: 0,
                    origin: Origin3d {
                        x: 0,
                        y: 0,
                        z: resident.slot,
                    },
                    aspect: TextureAspect::All,
                },
                &colours(&resident.mesh, &resident.presentation),
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(TILE_CELLS as u32 * 4),
                    rows_per_image: Some(TILE_CELLS as u32),
                },
                Extent3d {
                    width: TILE_CELLS as u32,
                    height: TILE_CELLS as u32,
                    depth_or_array_layers: 1,
                },
            );
        }
    }
}

/// Keeps surface RGB intact and expands per-column presentation over each owned cell.
pub(super) fn colours(mesh: &TileMesh, codes: &[u8]) -> Vec<u8> {
    let side = mesh.key.width_blocks() as usize / far_terrain::COLUMN_SIDE;
    let mut colours = mesh.colours.to_vec();
    for (index, cell) in colours.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let x = ((index % TILE_CELLS) << mesh.key.level) / far_terrain::COLUMN_SIDE;
        let z = ((index / TILE_CELLS) << mesh.key.level) / far_terrain::COLUMN_SIDE;
        cell[3] = codes[z * side + x];
    }
    colours
}
