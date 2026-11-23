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
//! run yet has its entries cleared instead, so a reused quad slot never shows its previous
//! occupant's tint. Replacing the cache, or any buffer the blend reads besides the arena streams
//! uploaded with each allocation, rebakes every resident allocation.

use bevy::{
    prelude::{Entity, FromWorld, Res, ResMut, Resource, World},
    render::{
        render_resource::{
            BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, Buffer,
            BufferBindingType, BufferDescriptor, BufferId, BufferInitDescriptor, BufferUsages,
            CachedComputePipelineId, ComputePassDescriptor, ComputePipelineDescriptor,
            PipelineCache, ShaderStages,
        },
        renderer::{RenderContext, RenderDevice},
    },
};
use render_scene::chunk::biome_tints::ChunkBiomeTintResourceIdentity;
use std::collections::HashMap;

/// Shader definition that enables cache reads in cube vertex programs.
pub(in crate::chunk) const SHADER_DEF: &str = "TERRAIN_TINT_CACHE";
/// Invocations per bake workgroup, substituted into the kernel as `TINT_BAKE_WORKGROUP_SIZE`.
pub(in crate::chunk) const WORKGROUP_SIZE: u32 = 64;
/// One `vec4<u32>`: gamma RGB bits and the valid flag.
const ENTRY_BYTES: u64 = 16;
/// Job words: first quad, quad count, origin slot and padding.
const JOB_WORDS: usize = 4;

/// Whether the cache fits a device with the separate terrain layout: one vertex storage slot
/// beyond `required_vertex_storage_buffers`, the opaque group plus the slot terrain units add,
/// so admitting the cache never costs unit batching its binding.
pub(in crate::chunk) fn supported(
    compact: bool,
    max_vertex_storage_buffers: u32,
    required_vertex_storage_buffers: u32,
) -> bool {
    !compact && max_vertex_storage_buffers > required_vertex_storage_buffers
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
        ty: storage(true),
        count: None,
    }
}

fn storage(read_only: bool) -> BindingType {
    BindingType::Buffer {
        ty: BufferBindingType::Storage { read_only },
        has_dynamic_offset: false,
        min_binding_size: None,
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

/// The cache, the zeroed binding used without one, and the allocations awaiting a bake.
pub(in crate::chunk) struct TintCacheState {
    cache: Option<TintCache>,
    /// A single zero entry, bound whenever the cache is absent, so every quad computes its tint
    /// and terrain never depends on the optional allocation.
    fallback: Buffer,
    /// The capacity and inputs whose allocation last failed, retried only when they change.
    failed: Option<(usize, TintInputs)>,
    pending: Vec<Entity>,
    bake_all: bool,
}

impl TintCacheState {
    /// Allocates the zeroed fallback binding.
    pub(in crate::chunk) fn new(device: &RenderDevice) -> Option<Self> {
        Some(Self {
            cache: None,
            fallback: create(device, "terrain tint cache fallback", 1)?,
            failed: None,
            pending: Vec::new(),
            bake_all: false,
        })
    }

    /// Records that `entity`'s allocation was uploaded and must be baked before it draws.
    pub(in crate::chunk) fn queue_bake(&mut self, entity: Entity) {
        if self.cache.is_some() && !self.bake_all {
            self.pending.push(entity);
        }
    }

    /// Keeps a cache for `quad_capacity` quads built from `inputs`, replacing one built for
    /// others and scheduling a full rebake, and returns the buffer to bind.
    pub(in crate::chunk) fn prepare(
        &mut self,
        device: &RenderDevice,
        quad_capacity: usize,
        inputs: TintInputs,
    ) -> &Buffer {
        let current = self
            .cache
            .as_ref()
            .is_some_and(|cache| cache.quad_capacity == quad_capacity && cache.inputs == inputs);
        if !current && self.failed != Some((quad_capacity, inputs)) {
            let entries = entry_count(
                quad_capacity,
                device.limits().max_storage_buffer_binding_size,
            );
            self.cache = create(device, "terrain tint cache", entries).map(|buffer| TintCache {
                buffer,
                entries,
                quad_capacity,
                inputs,
            });
            self.failed = self.cache.is_none().then_some((quad_capacity, inputs));
            self.pending.clear();
            self.bake_all = self.cache.is_some();
        }
        self.binding()
    }

    /// The cache, or the zeroed fallback while there is none.
    pub(in crate::chunk) fn binding(&self) -> &Buffer {
        self.cache
            .as_ref()
            .map_or(&self.fallback, |cache| &cache.buffer)
    }

    /// Bytes of the cache and its fallback, for arena telemetry.
    pub(in crate::chunk) fn capacity_bytes(&self) -> u64 {
        self.fallback.size() + self.cache.as_ref().map_or(0, |cache| cache.buffer.size())
    }

    /// Bake jobs for the pending allocations, or every resident one after a replacement, clipped
    /// to the cache's entries. Allocations replaced or removed since they were queued bake as
    /// they are now, or not at all.
    fn jobs<'a>(
        &self,
        allocations: impl Iterator<Item = (Entity, &'a super::types::GpuChunkAllocation)>,
    ) -> Vec<[u32; JOB_WORDS]> {
        let Some(cache) = &self.cache else {
            return Vec::new();
        };
        let allocations: HashMap<_, _> = allocations.collect();
        let mut entities: Vec<Entity> = if self.bake_all {
            allocations.keys().copied().collect()
        } else {
            self.pending.clone()
        };
        entities.sort_unstable();
        entities.dedup();
        entities
            .into_iter()
            .filter_map(|entity| allocations.get(&entity))
            .filter_map(|allocation| bake_job(allocation, cache.entries))
            .collect()
    }
}

/// The job baking `allocation`'s cube quads that fit `entries`, if any do.
fn bake_job(
    allocation: &super::types::GpuChunkAllocation,
    entries: u64,
) -> Option<[u32; JOB_WORDS]> {
    let end = u64::from(allocation.quad_range.end).min(entries) as u32;
    let start = allocation.quad_range.start.min(end);
    (start < end).then_some([start, end - start, allocation.metadata_index, 0])
}

/// Entries for `quad_capacity` quads, bounded by a binding of `max_binding_bytes`.
fn entry_count(quad_capacity: usize, max_binding_bytes: u64) -> u64 {
    (quad_capacity as u64)
        .min(max_binding_bytes / ENTRY_BYTES)
        .max(1)
}

/// A zeroed cache buffer of `entries`, or `None` when the device refuses the allocation.
fn create(device: &RenderDevice, label: &'static str, entries: u64) -> Option<Buffer> {
    crate::gpu_allocation::buffer(
        device,
        &BufferDescriptor {
            label: Some(label),
            size: entries * ENTRY_BYTES,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        },
    )
}

/// Resolves the bake kernel's generated constants.
pub(in crate::chunk) fn bake_shader_source(source: &str) -> String {
    crate::material_shader::source(source)
        .replace("TINT_BAKE_WORKGROUP_SIZE", &format!("{WORKGROUP_SIZE}u"))
}

/// Group 0 of the bake kernel: the packed-chunk tables it reads, at the chunk layout's numbers.
pub(in crate::chunk) fn source_layout() -> BindGroupLayoutDescriptor {
    let mut entries: Vec<_> = [1, 2, 3, 7, 8]
        .map(|binding| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::COMPUTE,
            ty: storage(true),
            count: None,
        })
        .into();
    entries.push(BindGroupLayoutEntry {
        binding: crate::material_shader::BIOME_QUERY_TABLES_BINDING,
        visibility: ShaderStages::COMPUTE,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    });
    BindGroupLayoutDescriptor::new("terrain tint bake sources", &entries)
}

/// Group 1 of the bake kernel: the jobs, then the cache it writes.
pub(in crate::chunk) fn output_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "terrain tint bake output",
        &[0, 1].map(|binding| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::COMPUTE,
            ty: storage(binding == 0),
            count: None,
        }),
    )
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
        let sources = source_layout();
        let output = output_layout();
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
    device: Res<RenderDevice>,
    sources: (
        Option<Res<crate::chunk::ChunkGpuTextureAssets>>,
        Option<Res<crate::chunk::ChunkGpuBiomeTints>>,
    ),
    mut context: RenderContext,
) {
    let (Some(mut arena), Some(pipeline)) = (arena, pipeline) else {
        return;
    };
    let arena = &mut *arena;
    let jobs = arena.tint_cache.jobs(
        arena
            .allocations
            .iter()
            .map(|(&entity, resident)| (entity, &resident.gpu)),
    );
    if jobs.is_empty() {
        arena.tint_cache.pending.clear();
        arena.tint_cache.bake_all = false;
        return;
    }
    let Some(target) = arena.tint_cache.cache.as_ref().map(|cache| &cache.buffer) else {
        return;
    };
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
    let encoder = context.command_encoder();
    let Some(((kernel, textures), tints)) = kernel else {
        // Zero entries make the vertex stage compute the tint; the bake retries next frame.
        for [start, count, ..] in &jobs {
            encoder.clear_buffer(
                target,
                u64::from(*start) * ENTRY_BYTES,
                Some(u64::from(*count) * ENTRY_BYTES),
            );
        }
        return;
    };
    let words: Vec<u32> = jobs.iter().flatten().copied().collect();
    let job_buffer = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("terrain tint bake jobs"),
        contents: bytemuck::cast_slice(&words),
        usage: BufferUsages::STORAGE,
    });
    let source_group = device.create_bind_group(
        "terrain tint bake sources",
        &cache.get_bind_group_layout(&pipeline.sources),
        &[
            (1, arena.quad_buffer.as_entire_binding()),
            (2, arena.origin_buffer.as_entire_binding()),
            (3, textures.material_buffer.as_entire_binding()),
            (7, arena.biome_buffer.as_entire_binding()),
            (8, tints.buffer.as_entire_binding()),
            (
                crate::material_shader::BIOME_QUERY_TABLES_BINDING,
                tints.query_tables.as_entire_binding(),
            ),
        ]
        .map(|(binding, resource)| BindGroupEntry { binding, resource }),
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
    let [groups_x, rows, layers] =
        dispatch_size(&jobs, device.limits().max_compute_workgroups_per_dimension);
    let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
        label: Some("terrain tint bake"),
        timestamp_writes: None,
    });
    pass.set_pipeline(kernel);
    pass.set_bind_group(0, &source_group, &[]);
    pass.set_bind_group(1, &output_group, &[]);
    pass.dispatch_workgroups(groups_x, rows, layers);
    drop(pass);
    arena.tint_cache.pending.clear();
    arena.tint_cache.bake_all = false;
}

/// Workgroups covering `jobs`: `x` runs over a job's quads, and `y + z * rows` selects the job
/// within a per-dimension `limit`.
fn dispatch_size(jobs: &[[u32; JOB_WORDS]], limit: u32) -> [u32; 3] {
    let widest = jobs.iter().map(|job| job[1]).max().unwrap_or(0);
    let count = jobs.len() as u32;
    let rows = count.min(limit);
    [
        widest.div_ceil(WORKGROUP_SIZE),
        rows,
        count.div_ceil(rows.max(1)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_cover_every_quad_within_the_binding_size() {
        assert_eq!(entry_count(100, u64::MAX), 100);
        assert_eq!(entry_count(100, 64 * ENTRY_BYTES + 15), 64);
        assert_eq!(entry_count(0, u64::MAX), 1);
    }

    #[test]
    fn the_cache_takes_a_slot_only_beside_the_unit_binding() {
        assert!(supported(false, 10, 9));
        assert!(!supported(false, 9, 9));
        assert!(!supported(true, 32, 9));
    }

    #[test]
    fn dispatch_reaches_every_job_and_quad_within_the_dimension_limit() {
        let jobs = vec![[0, 130, 0, 0], [200, 1, 1, 0], [300, 64, 2, 0]];
        assert_eq!(dispatch_size(&jobs, 65_535), [3, 3, 1]);
        assert_eq!(dispatch_size(&jobs, 2), [3, 2, 2]);
    }
}
