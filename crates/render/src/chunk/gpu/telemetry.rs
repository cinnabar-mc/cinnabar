//! Capacity-change telemetry, not a per-frame allocation or driver-memory estimate.

use crate::chunk::*;

/// Cumulative allocation outcomes, preserved when idle storage is replaced.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct ArenaMemoryTelemetry {
    pub(in crate::chunk) host_visible_attempts: u64,
    pub(in crate::chunk) host_visible_allocations: u64,
    pub(in crate::chunk) host_visible_bytes: u64,
    pub(in crate::chunk) idle_releases: u64,
}

impl ArenaMemoryTelemetry {
    /// Counts successful fallback bytes and warns once across all terrain arenas.
    pub(in crate::chunk::gpu) fn record_host_visible_allocation(
        &mut self,
        stream: &'static str,
        bytes: u64,
    ) {
        self.host_visible_allocations = self.host_visible_allocations.saturating_add(1);
        self.host_visible_bytes = self.host_visible_bytes.saturating_add(bytes);
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| {
            bevy::log::warn!(
                stream,
                bytes,
                "terrain growth is using host-visible GPU storage"
            );
        });
    }
}

pub(in crate::chunk::gpu) fn log_initial_arena_capacity(
    arena: &ChunkGpuArena,
    render_device: &RenderDevice,
) {
    let limits = render_device.limits();
    bevy::log::info!(
        max_buffer_size = limits.max_buffer_size,
        max_storage_buffer_binding_size = limits.max_storage_buffer_binding_size,
        "chunk GPU arena device limits (per buffer, not an aggregate memory budget)"
    );
    log_arena_capacity(arena, "initialized");
}

/// Logs only creation or capacity changes, at debug level: joins grow the arena dozens of times.
/// Reclaimed ranges retain capacity until the empty arena's readers complete.
pub(in crate::chunk) fn log_arena_capacity(arena: &ChunkGpuArena, event: &'static str) {
    let quad_bytes = arena.quad_buffer.size();
    let geometry_bytes = arena.geometry_stream_buffer.size();
    let origin_bytes = arena.origin_buffer.size();
    let biome_bytes = arena.biome_buffer.size();
    let auxiliary_bytes = total_capacity_bytes([
        arena.index_buffer.size(),
        arena.model_index_buffer.size(),
        arena.vertex_offset_buffer.size(),
        arena.indirect_buffer.size(),
        arena.transparent_indirect_buffer.size(),
        arena.transparent_ref_buffer.size(),
    ]);
    let migration_bytes = arena
        .migration
        .as_ref()
        .map_or(0, |migration| migration.buffer.size());
    let tracked_capacity_bytes = total_capacity_bytes([
        quad_bytes,
        geometry_bytes,
        origin_bytes,
        biome_bytes,
        auxiliary_bytes,
        migration_bytes,
    ]);
    bevy::log::debug!(
        event,
        quad_bytes,
        geometry_bytes,
        origin_bytes,
        biome_bytes,
        auxiliary_bytes,
        migration_bytes,
        tracked_capacity_bytes,
        allocations = arena.allocations.len(),
        retired_allocations = arena.retired_allocations.len(),
        retired_bytes = arena.retirement_budget.bytes,
        pending_removals = arena.pending_removals.len(),
        host_visible_attempts = arena.memory.host_visible_attempts,
        host_visible_allocations = arena.memory.host_visible_allocations,
        host_visible_bytes = arena.memory.host_visible_bytes,
        idle_releases = arena.memory.idle_releases,
        "chunk GPU arena capacity changed (excludes driver-retained submitted resources)"
    );
}

/// Sums tracked buffer capacity without overflowing diagnostic counters.
fn total_capacity_bytes<const N: usize>(bytes: [u64; N]) -> u64 {
    bytes.into_iter().fold(0, u64::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_accounting_includes_migration_without_overflow() {
        assert_eq!(total_capacity_bytes([8, 16, 32, 64, 128, 256]), 504);
        assert_eq!(total_capacity_bytes([u64::MAX, 1]), u64::MAX);
    }
}
