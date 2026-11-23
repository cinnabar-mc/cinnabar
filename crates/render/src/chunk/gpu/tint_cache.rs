//! Per-quad cache of the blended biome tint of one-block tinted cube faces.
//!
//! In a mixed biome record every tinted cube face is meshed one block wide, and its vertex stage
//! evaluates the 27-sample lattice blend for that block. Without a cache it repeats that blend in
//! all four vertices of every such face in every frame, which dominates opaque terrain cost.
//!
//! Slot 0 of the buffer holds the current epoch in `x`; slot `quad + 1` holds a quad's gamma RGB
//! bits and the epoch they were computed under. The provoking vertex trusts an entry only when
//! its epoch matches and otherwise recomputes and stores it, so the cached value is always the
//! value the same shader code would compute now. Advancing the epoch whenever a tint input may
//! have changed invalidates every entry at once.

use bevy::render::{
    render_resource::{
        BindGroupLayoutEntry, BindingType, Buffer, BufferBindingType, BufferDescriptor, BufferId,
        BufferUsages, ShaderStages,
    },
    renderer::{RenderDevice, RenderQueue},
};
use render_scene::chunk::biome_tints::ChunkBiomeTintResourceIdentity;

/// Shader definition that enables the cache in cube vertex programs.
pub(in crate::chunk) const SHADER_DEF: &str = "TERRAIN_TINT_CACHE";
/// One `vec4<u32>`: gamma RGB bits and the epoch, or the current epoch in slot 0.
const ENTRY_BYTES: u64 = 16;

/// Whether a device can write the cache from vertex programs with the separate terrain layout.
/// Compact layouts, sized for devices with few storage slots, never take the extra binding.
pub(in crate::chunk) fn supported(
    features: wgpu::Features,
    max_vertex_storage_buffers: u32,
    layout_vertex_storage_buffers: u32,
    compact: bool,
) -> bool {
    !compact
        && features.contains(wgpu::Features::VERTEX_WRITABLE_STORAGE)
        && layout_vertex_storage_buffers < max_vertex_storage_buffers
}

/// Reads only the diagnostic opt-out, `CINNABAR_TERRAIN_TINT_CACHE=0`, which keeps the uncached
/// blend for same-binary output comparisons.
pub(in crate::chunk) fn requested() -> bool {
    env_compat::var_os("CINNABAR_TERRAIN_TINT_CACHE").is_none_or(|value| value != "0")
}

/// The cache's entry in the opaque terrain bind group layout.
pub(in crate::chunk) fn layout_entry() -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding: crate::material_shader::TERRAIN_TINT_CACHE_BINDING,
        visibility: ShaderStages::VERTEX,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only: false },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Identities of the buffers whose contents feed a cached blend besides the arena streams,
/// whose writes advance [`TintCache::observe`]'s revision instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::chunk) struct TintInputs {
    pub(in crate::chunk) quads: BufferId,
    pub(in crate::chunk) origins: BufferId,
    pub(in crate::chunk) biomes: BufferId,
    pub(in crate::chunk) materials: BufferId,
    pub(in crate::chunk) biome_tints: BufferId,
    pub(in crate::chunk) biome_tint_table: ChunkBiomeTintResourceIdentity,
}

/// The cache buffer with the epoch its entries are checked against.
pub(in crate::chunk) struct TintCache {
    pub(in crate::chunk) buffer: Buffer,
    quad_capacity: usize,
    epoch: u32,
    observed: Option<(TintInputs, u64)>,
}

impl TintCache {
    /// Returns the cache for `quad_capacity` quads, replacing one sized for another capacity.
    /// A new buffer starts zeroed, and epoch zero is never current, so it holds no entry.
    /// Quads beyond the entries that fit the device's binding size or memory, down to none,
    /// take the uncached blend; `None` means not even the epoch slot could be allocated.
    pub(in crate::chunk) fn for_capacity<'a>(
        cache: &'a mut Option<Self>,
        device: &RenderDevice,
        quad_capacity: usize,
    ) -> Option<&'a mut Self> {
        if cache
            .as_ref()
            .is_none_or(|cache| cache.quad_capacity != quad_capacity)
        {
            let entries = entry_count(
                quad_capacity,
                device.limits().max_storage_buffer_binding_size,
            );
            let buffer = [entries, 1].into_iter().find_map(|entries| {
                crate::gpu_allocation::buffer(
                    device,
                    &BufferDescriptor {
                        label: Some("terrain tint cache"),
                        size: entries * ENTRY_BYTES,
                        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    },
                )
            })?;
            *cache = Some(Self {
                buffer,
                quad_capacity,
                epoch: 0,
                observed: None,
            });
        }
        cache.as_mut()
    }

    /// Advances and uploads the epoch unless `inputs` and the arena's tint `revision` both match
    /// the last observation. Call after every arena write of the frame has been issued.
    pub(in crate::chunk) fn observe(
        &mut self,
        queue: &RenderQueue,
        inputs: TintInputs,
        revision: u64,
    ) {
        if self.observed == Some((inputs, revision)) {
            return;
        }
        self.epoch = next_epoch(self.epoch);
        self.observed = Some((inputs, revision));
        queue.write_buffer(
            &self.buffer,
            0,
            bytemuck::cast_slice(&[self.epoch, 0, 0, 0]),
        );
    }
}

/// Entries for the epoch slot and `quad_capacity` quads, bounded by a binding of
/// `max_binding_bytes`.
fn entry_count(quad_capacity: usize, max_binding_bytes: u64) -> u64 {
    (quad_capacity as u64 + 1).min(max_binding_bytes / ENTRY_BYTES)
}

/// The epoch after `epoch`, skipping zero, which marks never-written entries.
fn next_epoch(epoch: u32) -> u32 {
    epoch.wrapping_add(1).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epochs_never_return_to_the_unwritten_marker() {
        assert_eq!(next_epoch(0), 1);
        assert_eq!(next_epoch(1), 2);
        assert_eq!(next_epoch(u32::MAX), 1);
    }

    #[test]
    fn entries_cover_every_quad_within_the_binding_size() {
        assert_eq!(entry_count(100, u64::MAX), 101);
        assert_eq!(entry_count(100, 64 * ENTRY_BYTES + 15), 64);
    }

    #[test]
    fn devices_without_vertex_storage_writes_or_slots_keep_the_uncached_blend() {
        let writable = wgpu::Features::VERTEX_WRITABLE_STORAGE;
        assert!(supported(writable, 9, 8, false));
        assert!(!supported(wgpu::Features::empty(), 9, 8, false));
        assert!(!supported(writable, 8, 8, false));
        assert!(!supported(writable, 9, 8, true));
    }
}
