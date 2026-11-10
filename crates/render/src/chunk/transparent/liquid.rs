use crate::chunk::*;

pub(in crate::chunk) fn transparent_liquid_phase_distance(
    rangefinder: &ViewRangefinder3d,
    key: SubChunkKey,
) -> f32 {
    transparent_model_phase_distance(rangefinder, key)
}

/// Identifies resident and retired allocations drawn by the complete ordered snapshot.
pub(in crate::chunk) fn transparent_frame_draws(
    snapshot: &TransparentOrderedSnapshot,
    arena: &ChunkGpuArena,
) -> Vec<(Entity, FrameAllocationIdentity)> {
    frame_draws_where(snapshot, arena, |_| true)
}

/// Identifies each allocation in a drawn reference range, including runs spanning chunks.
pub(in crate::chunk) fn transparent_frame_draws_for_range(
    snapshot: &TransparentOrderedSnapshot,
    arena: &ChunkGpuArena,
    ref_range: Range<u32>,
) -> Vec<(Entity, FrameAllocationIdentity)> {
    let refs = usize::try_from(ref_range.start)
        .ok()
        .zip(usize::try_from(ref_range.end).ok())
        .and_then(|(start, end)| snapshot.refs().get(start..end));
    let Some(refs) = refs.filter(|refs| !refs.is_empty()) else {
        return Vec::new();
    };
    let metadata = refs
        .iter()
        .map(|draw_ref| draw_ref.metadata_index())
        .collect::<HashSet<_>>();
    frame_draws_where(snapshot, arena, |index| metadata.contains(&index))
}

/// Filters draw evidence by metadata while retaining the snapshot's allocation identity checks.
fn frame_draws_where(
    snapshot: &TransparentOrderedSnapshot,
    arena: &ChunkGpuArena,
    includes_metadata: impl Fn(u32) -> bool,
) -> Vec<(Entity, FrameAllocationIdentity)> {
    let active = arena
        .allocations
        .iter()
        .map(|(&entity, allocation)| (entity, &allocation.gpu, true));
    let retired = arena
        .retired_allocations
        .iter()
        .map(|allocation| (allocation.entity, &allocation.identity, false));
    active
        .chain(retired)
        .filter_map(|(entity, allocation, active)| {
            (includes_metadata(allocation.metadata_index)
                && if active {
                    transparent_snapshot_references_resident_allocation(snapshot, allocation)
                } else {
                    transparent_snapshot_references_allocation(snapshot, allocation)
                })
            .then_some((
                entity,
                FrameAllocationIdentity {
                    entity,
                    key: allocation.key,
                    generation: allocation.generation,
                },
            ))
        })
        .collect()
}
