//! Publishes near admission and the far mask before any terrain draw lists are built.

use super::*;
use far_terrain::{ColumnPos, NearMask, TerrainHandoffs, TerrainPublication};
use render_scene::FarTerrainScene;

#[cfg(test)]
mod tests;

#[derive(Resource, Default)]
pub(crate) struct TerrainCoverage {
    pub(crate) state: TerrainPublication,
    handoffs: TerrainHandoffs,
    world: Option<render_scene::FarTerrainResidency>,
    source: Option<render_scene::terrain_publication::TerrainPublications>,
    source_revision: Option<u64>,
    enabled: bool,
    mask: Option<Arc<NearMask>>,
    applied_target: Option<Arc<far_terrain::TerrainTarget>>,
    far_revision: u64,
    dirty: bool,
    /// Set by a publication against a far target, which may hide allocations; cleared by
    /// the first walk without one, which admits them all again.
    hiding: bool,
}

impl TerrainCoverage {
    /// Marks actual GPU publication changes without rebuilding unchanged per-frame snapshots.
    pub(crate) fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// Invalidates empty proofs only after full geometry has actually published on the GPU.
    pub(crate) fn publish_full(&mut self, key: SubChunkKey, generation: u64) {
        if let Some(source) = &self.source {
            source.record(key, generation, false);
        }
        self.invalidate();
    }

    /// The mask uploaded by the same publication that admitted allocation components.
    #[cfg(feature = "publication-test-support")]
    pub(crate) fn mask(&self) -> Option<&NearMask> {
        self.mask.as_deref()
    }
}

/// Starts a separate publication epoch before this world's GPU completions are accepted.
pub(super) fn begin_terrain_coverage(
    scene: Option<Res<FarTerrainScene>>,
    mut coverage: ResMut<TerrainCoverage>,
    arena: Res<ChunkGpuArena>,
    gpu: Option<ResMut<crate::far_terrain::FarTerrainGpu>>,
) {
    let enabled = scene.as_ref().filter(|scene| scene.target.is_some());
    let source = scene.as_ref().and_then(|scene| scene.publications.as_ref());
    let same_world = match (source, coverage.source.as_ref()) {
        (Some(source), Some(previous)) => source.same_source(previous),
        (None, None) => match (enabled, coverage.world.as_ref()) {
            (Some(scene), Some(world)) => world.same_world(&scene.residency),
            (None, None) => true,
            _ => false,
        },
        _ => false,
    };
    if !same_world {
        coverage.state = TerrainPublication::default();
        coverage.source = source.cloned();
        coverage.source_revision = None;
    }
    if !same_world || coverage.enabled != enabled.is_some() {
        coverage.state.reset_draws();
        coverage.handoffs.clear();
        coverage.mask = None;
        coverage.applied_target = None;
        coverage.dirty = true;
    }
    coverage.enabled = enabled.is_some();
    coverage.world = enabled.map(|scene| scene.residency.clone());
    if let Some(mut gpu) = gpu {
        let protecting = coverage.state.far_columns().next().is_some();
        gpu.protect_coverage(
            arena
                .allocations
                .values()
                .take(if protecting { usize::MAX } else { 0 })
                .map(|allocation| allocation.gpu.key.chunk())
                .filter(|column| {
                    enabled.is_some_and(|scene| {
                        scene
                            .target
                            .as_ref()
                            .is_some_and(|target| target.contains(*column))
                    }) && (!coverage.state.draws_near(*column)
                        || coverage.handoffs.contains(*column))
                })
                .chain(coverage.state.far_columns().filter(|column| {
                    enabled.is_some_and(|scene| {
                        scene.target.as_ref().is_some_and(|target| {
                            target.contains(*column) && target.near.contains_key(column)
                        })
                    })
                }))
                .chain(coverage.handoffs.columns())
                .map(|column| ColumnPos::new(column.x, column.z)),
        );
    }
}

/// Admits every section of a column, its far mask and any near/far fade together. Without a
/// frame clock drawer changes are instant.
pub(super) fn publish_terrain_coverage(
    mut commands: Commands,
    scene: Option<Res<FarTerrainScene>>,
    mut coverage: ResMut<TerrainCoverage>,
    arena: Res<ChunkGpuArena>,
    allocations: Query<(), With<GpuChunkAllocation>>,
    mut gpu: Option<ResMut<crate::far_terrain::FarTerrainGpu>>,
    (device, queue, time): (Res<RenderDevice>, Res<RenderQueue>, Option<Res<Time>>),
) {
    // The extracted frame clock, shared with far preparation, so both read one instant.
    let clock = time.map(|time| time.elapsed());
    let target = scene.as_ref().and_then(|scene| scene.target.as_deref());
    let same_target = match (
        scene.as_ref().and_then(|scene| scene.target.as_ref()),
        coverage.applied_target.as_ref(),
    ) {
        (Some(target), Some(applied)) => Arc::ptr_eq(target, applied),
        (None, None) => true,
        _ => false,
    };
    // Only a far target reads empty proofs. Without one, copying them on every receipt is
    // O(loaded) waste; the next target is never `same_target`, so it replays them all.
    if target.is_some()
        && let Some((revision, empty)) = coverage.source.as_ref().and_then(|source| {
            source.changed_empty(if same_target {
                coverage.source_revision
            } else {
                None
            })
        })
    {
        coverage.state.replace_empty(empty);
        coverage.source_revision = Some(revision);
        coverage.dirty = true;
    }
    let far_revision = gpu.as_ref().map_or(0, |gpu| gpu.publication_revision());
    let ended = match clock {
        Some(now) => coverage.handoffs.advance(now, |column| {
            target.is_some_and(|target| target.contains(column))
        }),
        None => {
            let active = !coverage.handoffs.is_empty();
            coverage.handoffs.clear();
            active
        }
    };
    let now = clock.unwrap_or_default();
    if !coverage.dirty && same_target && coverage.far_revision == far_revision && !ended {
        if let Some(gpu) = gpu.as_mut() {
            gpu.publish_handoffs(&queue, handoff_codes(&coverage.handoffs, now), now);
        }
        return;
    }
    if let (Some(scene), Some(target), Some(gpu)) = (scene.as_ref(), target, gpu.as_mut()) {
        let live = arena
            .allocations
            .iter()
            .filter(|(entity, _)| !arena.pending_removals.contains(entity))
            .map(|(_, allocation)| (allocation.gpu.key, allocation.gpu.generation))
            .collect();
        let old_near: HashSet<_> = coverage.state.near_columns().collect();
        let complete_near: HashSet<_> = old_near
            .iter()
            .copied()
            .filter(|column| {
                coverage
                    .applied_target
                    .as_deref()
                    .is_some_and(|old| coverage.state.near_ready(*column, old, &live))
            })
            .collect();
        let changed = coverage.state.publish(target, &live, |column| {
            gpu.covers_column(ColumnPos::new(column.x, column.z))
        });
        let near: HashSet<_> = coverage.state.near_columns().collect();
        for column in old_near
            .symmetric_difference(&near)
            .copied()
            .filter(|column| clock.is_some() && target.contains(*column))
        {
            let predecessor_near = old_near.contains(&column);
            if (complete_near.contains(&column)
                || (!predecessor_near
                    && coverage
                        .applied_target
                        .as_deref()
                        .is_some_and(|old| old.contains(column))))
                && gpu.covers_column(ColumnPos::new(column.x, column.z))
            {
                coverage.handoffs.retarget(column, predecessor_near, now);
            }
        }
        gpu.publish_handoffs(&queue, handoff_codes(&coverage.handoffs, now), now);
        let view_changed = coverage.applied_target.as_ref().is_none_or(|applied| {
            applied.centre != target.centre || applied.horizon != target.horizon
        });
        if changed || ended || view_changed || coverage.mask.is_none() {
            let mask = NearMask::from_columns(
                [target.centre.x, target.centre.z],
                target.horizon,
                coverage
                    .state
                    .near_columns()
                    .filter(|column| !coverage.handoffs.contains(*column))
                    .filter(|column| column.dimension == target.centre.dimension)
                    .map(|column| ColumnPos::new(column.x, column.z)),
            );
            if coverage.mask.as_deref() != Some(&mask) {
                let mask = Arc::new(mask);
                gpu.publish_mask(&device, &queue, Arc::clone(&mask));
                coverage.mask = Some(mask);
            }
            scene.residency.publish_near(
                coverage
                    .state
                    .near_columns()
                    .chain(coverage.handoffs.codes(now).map(|(column, _)| column))
                    .filter(|column| column.dimension == target.centre.dimension)
                    .map(|column| ColumnPos::new(column.x, column.z)),
            );
        }
    }
    if target.is_none()
        && let Some(gpu) = gpu.as_mut()
    {
        gpu.publish_handoffs(&queue, std::iter::empty(), now);
    }
    // Allocation components are the common input of opaque, unit, indirect, GPU-cull and
    // transparent queues. Physical arena ownership and retirement remain unchanged. Upload
    // gives every new allocation its component, so without a far target, once a walk has
    // admitted whatever an earlier target hid, nothing needs admitting: the walk is skipped.
    if target.is_some() || coverage.hiding {
        for (&entity, allocation) in &arena.allocations {
            if arena.pending_removals.contains(&entity) {
                continue;
            }
            let admitted = target.is_none()
                || coverage.state.draws_near(allocation.gpu.key.chunk())
                || coverage.handoffs.contains(allocation.gpu.key.chunk());
            if admitted != allocations.contains(entity)
                && let Ok(mut entity) = commands.get_entity(entity)
            {
                if admitted {
                    entity.insert(allocation.gpu.clone());
                } else {
                    entity.remove::<GpuChunkAllocation>();
                }
            }
        }
    }
    coverage.hiding = target.is_some();
    coverage.dirty = false;
    coverage.far_revision = far_revision;
    coverage.applied_target = scene.as_ref().and_then(|scene| scene.target.clone());
}

/// Far-shader presentation codes of active near/far drawer changes.
fn handoff_codes(
    handoffs: &TerrainHandoffs,
    now: std::time::Duration,
) -> impl Iterator<Item = (ColumnPos, u32)> + '_ {
    handoffs
        .codes(now)
        .map(|(column, code)| (ColumnPos::new(column.x, column.z), code))
}
