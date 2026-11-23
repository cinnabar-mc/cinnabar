//! Per-quad cache of the blended biome tint of one-block tinted cube faces.
//!
//! In a mixed biome record every tinted cube face is meshed one block wide, and the vertex stage
//! evaluates the 27-sample lattice blend for that block. Without a cache it repeats that blend in
//! all four vertices of every such face in every frame, which dominates opaque terrain cost.
//!
//! A compute pass bakes entry `quad` of the cache, with the same shader code the vertex stage
//! would run, for the quads of every allocation uploaded since the last bake: the gamma RGB bits
//! and 1 in `w` when the face takes the blend, zero otherwise. Vertex programs only read the
//! cache and compute the blend themselves for any zero entry. An allocation whose bake cannot
//! run has its entries cleared instead, so a reused quad slot never shows its previous
//! occupant's tint. Replacing the cache, or any buffer the blend reads besides the arena streams
//! uploaded with each allocation, rebakes every resident allocation. Every cache allocation is
//! optional: without one, the opaque group binds a view whose single entry is never valid.

use super::tint_bake_plan::{self as plan, ENTRY_BYTES, Job};
use bevy::{
    prelude::{Entity, FromWorld, Res, ResMut, Resource, World},
    render::{
        render_resource::{
            BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingResource,
            BindingType, Buffer, BufferBinding, BufferBindingType, BufferDescriptor, BufferId,
            BufferUsages, CachedComputePipelineId, ComputePassDescriptor,
            ComputePipelineDescriptor, PipelineCache, ShaderStages,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
    },
};
use render_scene::chunk::biome_tints::ChunkBiomeTintResourceIdentity;
use std::collections::HashMap;

/// Shader definition that enables cache reads in cube vertex programs.
pub(in crate::chunk) const SHADER_DEF: &str = "TERRAIN_TINT_CACHE";

// Without a cache, the opaque group binds the first entry's bytes of the biome buffer, which
// always holds the fallback record. Its `w` word must never read as a valid entry.
const _: () = assert!(
    crate::chunk::FALLBACK_BIOME_RECORD.len() * 4 >= ENTRY_BYTES as usize
        && crate::chunk::FALLBACK_BIOME_RECORD[3] != 1
);

/// Whether the cache fits a device: the separate terrain layout with one vertex storage slot
/// beyond `required_vertex_storage_buffers` (the opaque group plus the slot terrain units add,
/// so the cache never costs unit batching its binding), and everything the bake kernel needs.
pub(in crate::chunk) fn supported(
    compact: bool,
    downlevel: wgpu::DownlevelFlags,
    limits: &wgpu::Limits,
    required_vertex_storage_buffers: u32,
) -> bool {
    !compact
        && limits.max_storage_buffers_per_shader_stage > required_vertex_storage_buffers
        && plan::kernel_supported(downlevel, limits)
}

/// Reads only the diagnostic opt-out, `CINNABAR_TERRAIN_TINT_CACHE=0`, which keeps the uncached
/// blend for same-binary output comparisons.
pub(in crate::chunk) fn requested() -> bool {
    env_compat::var_os("CINNABAR_TERRAIN_TINT_CACHE").is_none_or(|value| value != "0")
}

/// The cache's read-only entry in the opaque terrain bind group layout.
pub(in crate::chunk) fn layout_entry() -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding: crate::material_shader::TERRAIN_TINT_CACHE_BINDING,
        visibility: ShaderStages::VERTEX,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Identities of the buffers a bake reads besides the per-allocation arena streams.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::chunk) struct TintInputs {
    pub(in crate::chunk) materials: BufferId,
    pub(in crate::chunk) biome_tints: BufferId,
    pub(in crate::chunk) biome_tint_table: ChunkBiomeTintResourceIdentity,
}

struct TintCache {
    buffer: Buffer,
    /// Entries the buffer holds, at most the arena's quad capacity.
    entries: u64,
    quad_capacity: usize,
    inputs: TintInputs,
}

/// The cache and the allocations awaiting a bake.
#[derive(Default)]
pub(in crate::chunk) struct TintCacheState {
    cache: Option<TintCache>,
    /// The capacity and inputs whose allocation last failed, retried only when they change.
    failed: Option<(usize, TintInputs)>,
    pending: Vec<Entity>,
    bake_all: bool,
}

impl TintCacheState {
    /// Records that `entity`'s allocation was uploaded and must be baked before it draws.
    pub(in crate::chunk) fn queue_bake(&mut self, entity: Entity) {
        if self.cache.is_some() && !self.bake_all {
            self.pending.push(entity);
        }
    }

    /// Keeps a cache for `quad_capacity` quads built from `inputs`, replacing one built for
    /// others and scheduling a full rebake. A failed allocation leaves no cache.
    pub(in crate::chunk) fn prepare(
        &mut self,
        device: &RenderDevice,
        quad_capacity: usize,
        inputs: TintInputs,
    ) {
        let current = self
            .cache
            .as_ref()
            .is_some_and(|cache| cache.quad_capacity == quad_capacity && cache.inputs == inputs);
        if current || self.failed == Some((quad_capacity, inputs)) {
            return;
        }
        let entries = plan::entry_count(quad_capacity, &device.limits());
        self.cache = crate::gpu_allocation::buffer(
            device,
            &BufferDescriptor {
                label: Some("terrain tint cache"),
                size: entries * ENTRY_BYTES,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            },
        )
        .map(|buffer| TintCache {
            buffer,
            entries,
            quad_capacity,
            inputs,
        });
        self.failed = self.cache.is_none().then_some((quad_capacity, inputs));
        self.pending.clear();
        self.bake_all = self.cache.is_some();
    }

    /// The cache's identity, for bind group rebuilds.
    pub(in crate::chunk) fn id(&self) -> Option<BufferId> {
        self.cache.as_ref().map(|cache| cache.buffer.id())
    }

    /// The cache, or else the fallback record at the head of `biome_buffer` as one entry that is
    /// never valid, so every quad computes its tint and terrain never depends on the cache.
    pub(in crate::chunk) fn binding<'a>(&'a self, biome_buffer: &'a Buffer) -> BindingResource<'a> {
        match &self.cache {
            Some(cache) => cache.buffer.as_entire_binding(),
            None => BindingResource::Buffer(BufferBinding {
                buffer: biome_buffer,
                offset: 0,
                size: std::num::NonZeroU64::new(ENTRY_BYTES),
            }),
        }
    }

    /// Bytes of the cache, for arena telemetry.
    pub(in crate::chunk) fn capacity_bytes(&self) -> u64 {
        self.cache.as_ref().map_or(0, |cache| cache.buffer.size())
    }

    /// Whether a bake is owed.
    fn has_work(&self) -> bool {
        self.cache.is_some() && (self.bake_all || !self.pending.is_empty())
    }

    /// Bake jobs for the pending allocations, or every resident one after a replacement, clipped
    /// to the cache's entries. Allocations replaced or removed since they were queued bake as
    /// they are now, or not at all.
    fn jobs(&self, allocations: &HashMap<Entity, super::types::ArenaAllocation>) -> Vec<Job> {
        let Some(cache) = &self.cache else {
            return Vec::new();
        };
        let job = |resident: &super::types::ArenaAllocation| {
            plan::job(
                resident.gpu.quad_range.clone(),
                resident.gpu.metadata_index,
                cache.entries,
            )
        };
        if self.bake_all {
            return allocations.values().filter_map(job).collect();
        }
        let mut pending = self.pending.clone();
        pending.sort_unstable();
        pending.dedup();
        pending
            .iter()
            .filter_map(|entity| allocations.get(entity))
            .filter_map(job)
            .collect()
    }

    fn settle(&mut self) {
        self.pending.clear();
        self.bake_all = false;
    }
}

/// The bake kernel, queued only when the opaque pipelines read the cache.
#[derive(Resource)]
pub(in crate::chunk) struct TintBakePipeline {
    id: Option<CachedComputePipelineId>,
    sources: BindGroupLayoutDescriptor,
    output: BindGroupLayoutDescriptor,
}

impl FromWorld for TintBakePipeline {
    fn from_world(world: &mut World) -> Self {
        let enabled = world
            .resource::<crate::chunk::pipeline::layouts::ChunkPipeline>()
            .tint_cache;
        let sources = BindGroupLayoutDescriptor::new(
            "terrain tint bake sources",
            &plan::source_layout_entries(),
        );
        let output = BindGroupLayoutDescriptor::new(
            "terrain tint bake output",
            &plan::output_layout_entries(),
        );
        let id = enabled.then(|| {
            world
                .resource::<PipelineCache>()
                .queue_compute_pipeline(ComputePipelineDescriptor {
                    label: Some("terrain tint bake".into()),
                    layout: vec![sources.clone(), output.clone()],
                    shader: crate::chunk::TINT_BAKE_SHADER_HANDLE,
                    entry_point: Some("bake".into()),
                    ..Default::default()
                })
        });
        Self {
            id,
            sources,
            output,
        }
    }
}

/// Bakes, or else clears, the pending allocations' entries ahead of every camera pass.
pub(in crate::chunk) fn record_tint_bake(
    arena: Option<ResMut<crate::chunk::ChunkGpuArena>>,
    pipeline: Option<Res<TintBakePipeline>>,
    cache: Res<PipelineCache>,
    gpu: (Res<RenderDevice>, Res<RenderQueue>),
    sources: (
        Option<Res<crate::chunk::ChunkGpuTextureAssets>>,
        Option<Res<crate::chunk::ChunkGpuBiomeTints>>,
    ),
    mut context: RenderContext,
) {
    let (Some(mut arena), Some(pipeline)) = (arena, pipeline) else {
        return;
    };
    if !arena.tint_cache.has_work() {
        return;
    }
    let arena = &mut *arena;
    let jobs = arena.tint_cache.jobs(&arena.allocations);
    let Some(target) = arena.tint_cache.cache.as_ref().map(|cache| &cache.buffer) else {
        return;
    };
    let (device, queue) = gpu;
    let limit = device.limits().max_compute_workgroups_per_dimension;
    let (planned, deferred) = plan::plan(&jobs, limit);
    let encoder = context.command_encoder();
    // Entries a bake cannot reach this frame compute in the vertex stage until it does.
    plan::clear(encoder, target, &deferred);
    let (textures, tints) = sources;
    let kernel = pipeline
        .id
        .and_then(|id| cache.get_compute_pipeline(id))
        .zip(
            textures
                .as_deref()
                .and_then(|textures| textures.prepared.as_ref()),
        )
        .zip(tints.as_deref().and_then(|tints| tints.prepared.as_ref()));
    let words: Vec<u32> = planned.iter().flatten().copied().collect();
    let job_buffer = kernel.and(crate::gpu_allocation::buffer_with_data(
        &device,
        &queue,
        "terrain tint bake jobs",
        bytemuck::cast_slice(&words),
        BufferUsages::STORAGE,
    ));
    let (Some(((kernel, textures), tints)), Some(job_buffer)) = (kernel, job_buffer) else {
        plan::clear(encoder, target, &planned);
        return;
    };
    let source_group = device.create_bind_group(
        "terrain tint bake sources",
        &cache.get_bind_group_layout(&pipeline.sources),
        &[
            arena.quad_buffer.as_entire_binding(),
            arena.origin_buffer.as_entire_binding(),
            textures.material_buffer.as_entire_binding(),
            arena.biome_buffer.as_entire_binding(),
            tints.buffer.as_entire_binding(),
        ]
        .into_iter()
        .zip(plan::SOURCE_STORAGE_BINDINGS)
        .chain([(
            tints.query_tables.as_entire_binding(),
            crate::material_shader::BIOME_QUERY_TABLES_BINDING,
        )])
        .map(|(resource, binding)| BindGroupEntry { binding, resource })
        .collect::<Vec<_>>(),
    );
    let output_group = device.create_bind_group(
        "terrain tint bake output",
        &cache.get_bind_group_layout(&pipeline.output),
        &[
            BindGroupEntry {
                binding: 0,
                resource: job_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: target.as_entire_binding(),
            },
        ],
    );
    {
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("terrain tint bake"),
            timestamp_writes: None,
        });
        pass.set_pipeline(kernel);
        pass.set_bind_group(0, &source_group, &[]);
        pass.set_bind_group(1, &output_group, &[]);
        plan::dispatch(&mut pass, &planned, limit);
    }
    if deferred.is_empty() {
        arena.tint_cache.settle();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cache_needs_a_slot_beside_the_unit_binding_and_the_kernel() {
        let compute = wgpu::DownlevelFlags::COMPUTE_SHADERS;
        let limits = |slots| wgpu::Limits {
            max_storage_buffers_per_shader_stage: slots,
            ..wgpu::Limits::default()
        };
        assert!(supported(false, compute, &limits(10), 9));
        assert!(!supported(false, compute, &limits(9), 9));
        assert!(!supported(true, compute, &limits(32), 9));
        assert!(!supported(
            false,
            wgpu::DownlevelFlags::empty(),
            &limits(32),
            9
        ));
    }
}
