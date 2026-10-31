//! Checked construction and memory placement for terrain buffers.

use super::arena::{INITIAL_ARENA_CAPACITIES, INITIAL_INDIRECT_CAPACITY};
use super::telemetry::ArenaMemoryTelemetry;
use crate::chunk::*;

/// Shared allocation label for identity-offset storage and its recovery telemetry.
pub(in crate::chunk::gpu) const IDENTITY_VERTEX_OFFSETS_LABEL: &str =
    "terrain identity vertex offsets";

/// Tries ordinary storage, then one host-visible allocation when the device enables it.
/// Reports placement separately so callers count success only after their transaction commits.
pub(super) fn terrain_buffer(
    device: &RenderDevice,
    mut descriptor: BufferDescriptor<'static>,
    memory: &mut ArenaMemoryTelemetry,
) -> Option<(Buffer, bool)> {
    if let Some(buffer) = crate::gpu_allocation::buffer(device, &descriptor) {
        return Some((buffer, false));
    }
    if !device
        .features()
        .contains(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS)
    {
        return None;
    }
    memory.host_visible_attempts = memory.host_visible_attempts.saturating_add(1);
    descriptor.usage |= BufferUsages::MAP_WRITE;
    crate::gpu_allocation::buffer(device, &descriptor).map(|buffer| (buffer, true))
}

/// Validates allocation before touching its initial mapping, then leaves the buffer unmapped.
fn initialize(buffer: &Buffer, contents: &[u8]) -> Option<()> {
    buffer
        .get_mapped_range_mut(..)
        .ok()?
        .slice(..contents.len())
        .copy_from_slice(contents);
    buffer.unmap();
    Some(())
}

/// Creates a nonempty initialized buffer without mapping a failed allocation.
fn initialized_buffer(device: &RenderDevice, init: &BufferInitDescriptor<'_>) -> Option<Buffer> {
    let buffer = crate::gpu_allocation::buffer(
        device,
        &BufferDescriptor {
            label: init.label,
            size: (init.contents.len() as u64).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT),
            usage: init.usage,
            mapped_at_creation: true,
        },
    )?;
    initialize(&buffer, init.contents)?;
    Some(buffer)
}

/// Checks the minimal indirect allocation before it can be installed in a replacement arena.
fn initial_indirect_buffer(device: &RenderDevice) -> Option<Buffer> {
    crate::gpu_allocation::checked(
        device,
        "initial terrain indirect storage",
        INITIAL_INDIRECT_CAPACITY as u64 * INDEXED_INDIRECT_BYTES,
        || create_indirect_buffer(device, INITIAL_INDIRECT_CAPACITY),
    )
}

/// Vertex slot `i` contains `(i, 0)` for CPU-planned draws; GPU culling supplies its own offsets.
fn identity_vertex_offsets(origin_capacity: usize) -> Vec<u32> {
    let corners = crate::chunk::gpu_cull::model::OFFSET_CORNERS;
    (0..origin_capacity as u32 * corners)
        .flat_map(|vertex| [vertex, 0])
        .collect()
}

/// Prepares the origin stream's companion buffer, using the same fallible placement policy.
pub(super) fn try_identity_vertex_offsets(
    device: &RenderDevice,
    origin_capacity: usize,
    memory: &mut ArenaMemoryTelemetry,
) -> Option<(Buffer, bool)> {
    let entries = identity_vertex_offsets(origin_capacity);
    let bytes = bytemuck::cast_slice(&entries);
    let (buffer, host_visible) = terrain_buffer(
        device,
        BufferDescriptor {
            label: Some(IDENTITY_VERTEX_OFFSETS_LABEL),
            size: bytes.len() as u64,
            usage: BufferUsages::VERTEX,
            mapped_at_creation: true,
        },
        memory,
    )?;
    initialize(&buffer, bytes)?;
    Some((buffer, host_visible))
}

impl ChunkGpuArena {
    /// Allocates the startup arena, failing explicitly if even minimal storage is unavailable.
    pub(in crate::chunk) fn new(render_device: &RenderDevice) -> Self {
        let arena = Self::try_new(render_device).expect("allocate initial terrain GPU arena");
        super::telemetry::log_initial_arena_capacity(&arena, render_device);
        arena
    }

    /// Builds a complete minimal replacement without exposing partially allocated buffers.
    pub(in crate::chunk::gpu) fn try_new(render_device: &RenderDevice) -> Option<Self> {
        let device_limits = render_device.limits();
        let limits = arena_limits_from_device_limits(
            device_limits.max_buffer_size,
            device_limits.max_storage_buffer_binding_size,
        );
        let arena = Self {
            quad_buffer: super::arena::try_create_storage_buffer(
                render_device,
                "packed chunk quads",
                INITIAL_ARENA_CAPACITIES.quads as u64 * PACKED_QUAD_BYTES,
            )?,
            geometry_stream_buffer: super::arena::try_create_storage_buffer(
                render_device,
                "packed chunk geometry streams",
                INITIAL_ARENA_CAPACITIES.geometry_stream_words as u64 * GEOMETRY_STREAM_WORD_BYTES,
            )?,
            origin_buffer: super::arena::try_create_storage_buffer(
                render_device,
                "packed chunk origins",
                INITIAL_ARENA_CAPACITIES.origins as u64 * CHUNK_ORIGIN_BYTES,
            )?,
            biome_buffer: initialized_buffer(
                render_device,
                &BufferInitDescriptor {
                    label: Some("packed chunk biome records"),
                    contents: bytemuck::cast_slice(&FALLBACK_BIOME_RECORD),
                    usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
                },
            )?,
            index_buffer: initialized_buffer(
                render_device,
                &BufferInitDescriptor {
                    label: Some("shared chunk quad indices"),
                    contents: bytemuck::cast_slice(&STATIC_QUAD_INDICES),
                    usage: BufferUsages::INDEX,
                },
            )?,
            model_index_buffer: initialized_buffer(
                render_device,
                &BufferInitDescriptor {
                    label: Some("shared model template quad indices"),
                    contents: bytemuck::cast_slice(&STATIC_QUAD_INDICES),
                    usage: BufferUsages::INDEX,
                },
            )?,
            vertex_offset_buffer: initialized_buffer(
                render_device,
                &BufferInitDescriptor {
                    label: Some(IDENTITY_VERTEX_OFFSETS_LABEL),
                    contents: bytemuck::cast_slice(&identity_vertex_offsets(
                        INITIAL_ARENA_CAPACITIES.origins,
                    )),
                    usage: BufferUsages::VERTEX,
                },
            )?,
            indirect_buffer: initial_indirect_buffer(render_device)?,
            transparent_indirect_buffer: initial_indirect_buffer(render_device)?,
            transparent_ref_buffer: crate::gpu_allocation::checked(
                render_device,
                "initial terrain reference storage",
                transparent_ref_offset(2, INITIAL_TRANSPARENT_SLOT_REFS, 0),
                || transparent_ref_buffer(render_device, INITIAL_TRANSPARENT_SLOT_REFS),
            )?,
            transparent_slot_refs: INITIAL_TRANSPARENT_SLOT_REFS,
            bind_group: None,
            transparent_bind_group: None,
            bind_group_buffers: None,
            quad_capacity: INITIAL_ARENA_CAPACITIES.quads,
            geometry_stream_capacity: INITIAL_ARENA_CAPACITIES.geometry_stream_words,
            origin_capacity: INITIAL_ARENA_CAPACITIES.origins,
            biome_capacity: INITIAL_ARENA_CAPACITIES.biome_words,
            indirect_capacity: INITIAL_INDIRECT_CAPACITY,
            uploaded_indirect_bytes: Vec::new(),
            quad_len: 0,
            geometry_stream_len: 0,
            origin_len: 0,
            biome_len: FALLBACK_BIOME_WORDS,
            limits,
            free_quads: FreeRanges::default(),
            free_geometry_stream_words: FreeRanges::default(),
            free_origins: Vec::new(),
            free_biomes: FreeRanges::default(),
            shared_biome_ranges: SharedBiomeRanges::default(),
            allocations: HashMap::new(),
            transparent_liquids: TransparentLiquidResidents::default(),
            // A new arena may replace one whose allocations the orders still witness.
            transparent_model_changes: TransparentModelResidentChanges::all_changed(),
            retired_allocations: Vec::new(),
            pending_removals: BTreeSet::new(),
            retirement_budget: TransparentRetirementBudget::with_limits(
                MAX_TRANSPARENT_RETIRED_ALLOCATIONS,
                MAX_TRANSPARENT_RETIRED_BYTES,
            ),
            migration: None,
            memory: Default::default(),
            idle_completion: None,
        };
        Some(arena)
    }
}
