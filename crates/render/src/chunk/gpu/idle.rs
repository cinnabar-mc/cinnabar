//! Returns an empty arena's high-water storage after its submitted readers complete.

use crate::chunk::*;
use std::sync::atomic::{AtomicBool, Ordering};

/// Releases storage only without live, retained, migrating or newly queued geometry.
/// Dropping buffers rather than destroying them also preserves any external wgpu references.
pub(in crate::chunk) fn release_idle_arena(
    arena: &mut ChunkGpuArena,
    device: &RenderDevice,
    queue: &RenderQueue,
    pending_uploads: bool,
) {
    if pending_uploads
        || !arena.allocations.is_empty()
        || !arena.pending_removals.is_empty()
        || !arena.retired_allocations.is_empty()
        || arena.migration.is_some()
    {
        arena.idle_completion = None;
        return;
    }
    if arena_capacities(arena) == super::arena::INITIAL_ARENA_CAPACITIES
        && arena.indirect_capacity == super::arena::INITIAL_INDIRECT_CAPACITY
        && arena.transparent_slot_refs == INITIAL_TRANSPARENT_SLOT_REFS
    {
        return;
    }
    let Some(completed) = arena.idle_completion.as_ref() else {
        let completed = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&completed);
        // Covers submitted draws as well as the final migration and upload copies.
        queue.on_submitted_work_done(move || signal.store(true, Ordering::Release));
        arena.idle_completion = Some(completed);
        return;
    };
    if !completed.load(Ordering::Acquire) {
        return;
    }
    let mut memory = arena.memory;
    memory.idle_releases = memory.idle_releases.saturating_add(1);
    // Replace bindings, auxiliary buffers and free lists together. Atlas ownership and
    // retirement epochs live outside the arena and keep their existing publication order.
    *arena = ChunkGpuArena::new(device);
    arena.memory = memory;
    super::telemetry::log_arena_capacity(arena, "idle storage released");
}
