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

/// Whether the cache fits a device: a native backend, the separate terrain layout with one
/// vertex storage slot beyond `required_vertex_storage_buffers` (the opaque group plus the slot
/// terrain units add, so the cache never costs unit batching its binding), room for its binding
/// number in the opaque group, and everything the bake kernel needs.
pub(in crate::chunk) fn supported(
    compact: bool,
    downlevel: wgpu::DownlevelFlags,
    limits: &wgpu::Limits,
    required_vertex_storage_buffers: u32,
) -> bool {
    // Browsers report allocation failures asynchronously, so an optional cache cannot fall back.
    !cfg!(target_arch = "wasm32")
        && !compact
        && limits.max_storage_buffers_per_shader_stage > required_vertex_storage_buffers
        && limits.max_bindings_per_bind_group > crate::material_shader::TERRAIN_TINT_CACHE_BINDING
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
    /// The capacity and inputs whose cache last failed or was released, retried only on change.
    failed: Option<(usize, TintInputs)>,
    /// Allocations owed a bake, with how many of their quads earlier frames already baked.
    pending: HashMap<Entity, u32>,
    bake_all: bool,
}

/// One frame's bake: jobs one dispatch reaches, entries still waiting to be cleared, and the
/// progress to record once the jobs are recorded.
struct Batch {
    jobs: Vec<Job>,
    waiting: Vec<Job>,
    progress: Vec<(Entity, Option<u32>)>,
}

impl TintCacheState {
    /// Records that `entity`'s allocation was uploaded and must be baked from its first quad.
    pub(in crate::chunk) fn queue_bake(&mut self, entity: Entity) {
        if self.cache.is_some() && !self.bake_all {
            self.pending.insert(entity, 0);
        }
    }

    /// Frees the cache for required terrain storage. It is not recreated until the arena's quad
    /// capacity or the tint tables change, and bind groups fall back to the never-valid alias.
    /// Returns whether there was a cache to release.
    pub(in crate::chunk) fn release_for_required_storage(&mut self) -> bool {
        let Some(cache) = self.cache.take() else {
            return false;
        };
        self.failed = Some((cache.quad_capacity, cache.inputs));
        self.pending.clear();
        self.bake_all = false;
        true
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
    pub(in crate::chunk) fn has_work(&self) -> bool {
        self.cache.is_some() && (self.bake_all || !self.pending.is_empty())
    }

    /// Plans this frame's bake within `limit` workgroups per axis. Allocations that departed,
    /// hold no cube quads or lie beyond the cache are dropped from the queue now; the others bake
    /// in queue order up to what one dispatch reaches, and the rest wait with cleared entries.
    fn plan_batch(
        &mut self,
        allocations: &HashMap<Entity, super::types::ArenaAllocation>,
        limit: u32,
    ) -> Batch {
        let Some(entries) = self.cache.as_ref().map(|cache| cache.entries) else {
            return Batch {
                jobs: Vec::new(),
                waiting: Vec::new(),
                progress: Vec::new(),
            };
        };
        if std::mem::take(&mut self.bake_all) {
            self.pending = allocations.keys().map(|&entity| (entity, 0)).collect();
        }
        plan_owed(
            &mut self.pending,
            |entity| {
                allocations
                    .get(&entity)
                    .map(|resident| (resident.gpu.quad_range.clone(), resident.gpu.metadata_index))
            },
            entries,
            limit,
        )
    }

    /// Retires the work `batch` recorded: finished allocations leave the queue and partial ones
    /// keep their progress.
    fn commit(&mut self, batch: &Batch) {
        for &(entity, progress) in &batch.progress {
            match progress {
                Some(done) => {
                    self.pending.insert(entity, done);
                }
                None => {
                    self.pending.remove(&entity);
                }
            }
        }
    }
}

/// Plans one dispatch over `pending`, where `resident` gives each live allocation's quads and
/// origin slot. Entries that departed, hold no quads or lie beyond `entries` leave `pending`;
/// the rest bake in entity order up to what one dispatch reaches within `limit`, from the quads
/// earlier frames already baked, and the remainder waits.
fn plan_owed(
    pending: &mut HashMap<Entity, u32>,
    resident: impl Fn(Entity) -> Option<(std::ops::Range<u32>, u32)>,
    entries: u64,
    limit: u32,
) -> Batch {
    let remaining = |entity: Entity, done: u32| {
        resident(entity).and_then(|(quads, metadata)| {
            plan::job(
                quads.start.saturating_add(done)..quads.end,
                metadata,
                entries,
            )
        })
    };
    pending.retain(|&entity, &mut done| remaining(entity, done).is_some());
    let mut owed: Vec<_> = pending
        .iter()
        .map(|(&entity, &done)| (entity, done))
        .collect();
    owed.sort_unstable();
    let mut batch = Batch {
        jobs: Vec::new(),
        waiting: Vec::new(),
        progress: Vec::new(),
    };
    let reach = plan::reach(limit);
    for (entity, done) in owed {
        let Some(job) = remaining(entity, done) else {
            continue;
        };
        let mut baked = 0;
        for piece in plan::split(job, limit) {
            if batch.jobs.len() < reach {
                baked += piece[1];
                batch.jobs.push(piece);
            } else {
                batch.waiting.push(piece);
            }
        }
        if baked > 0 {
            let finished = baked == job[1];
            batch
                .progress
                .push((entity, (!finished).then_some(done + baked)));
        }
    }
    batch
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

/// Whether [`record_tint_bake`] has work: idle frames skip the system and its parameters.
pub(in crate::chunk) fn tint_bake_owed(arena: Option<Res<crate::chunk::ChunkGpuArena>>) -> bool {
    arena.is_some_and(|arena| arena.tint_cache.has_work())
}

/// Bakes the owed entries one dispatch reaches ahead of every camera pass, and clears the rest
/// and any whose bake cannot be recorded, so the vertex stage computes them until they bake.
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
    let Some(mut arena) = arena else {
        return;
    };
    let arena = &mut *arena;
    let (device, queue) = gpu;
    let limit = device.limits().max_compute_workgroups_per_dimension;
    let batch = arena.tint_cache.plan_batch(&arena.allocations, limit);
    let Some(target) = arena.tint_cache.cache.as_ref().map(|cache| &cache.buffer) else {
        return;
    };
    if batch.jobs.is_empty() {
        // Every owed allocation was dropped as departed, empty or beyond the cache.
        return;
    }
    let encoder = context.command_encoder();
    plan::clear(encoder, target, &batch.waiting);
    let (textures, tints) = sources;
    let kernel = pipeline
        .as_deref()
        .and_then(|pipeline| pipeline.id.and_then(|id| cache.get_compute_pipeline(id)))
        .zip(
            textures
                .as_deref()
                .and_then(|textures| textures.prepared.as_ref()),
        )
        .zip(tints.as_deref().and_then(|tints| tints.prepared.as_ref()));
    let job_buffer = kernel.and_then(|_| {
        let words: Vec<u32> = batch.jobs.iter().flatten().copied().collect();
        crate::gpu_allocation::buffer_with_data(
            &device,
            &queue,
            "terrain tint bake jobs",
            bytemuck::cast_slice(&words),
            BufferUsages::STORAGE,
        )
    });
    let (Some(pipeline), Some(((kernel, textures), tints)), Some(job_buffer)) =
        (pipeline, kernel, job_buffer)
    else {
        // The queue keeps the work; its entries stay cleared until a bake can be recorded.
        plan::clear(encoder, target, &batch.jobs);
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
        plan::dispatch(&mut pass, &batch.jobs, limit);
    }
    arena.tint_cache.commit(&batch);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(index: u32) -> Entity {
        Entity::from_raw_u32(index).unwrap()
    }

    /// Applies `batch` to `pending` as a recorded bake does.
    fn commit(pending: &mut HashMap<Entity, u32>, batch: &Batch) {
        for &(entity, progress) in &batch.progress {
            match progress {
                Some(done) => pending.insert(entity, done),
                None => pending.remove(&entity),
            };
        }
    }

    #[test]
    fn owed_work_drops_departed_empty_and_uncached_allocations_without_a_dispatch() {
        let mut pending = HashMap::from([(entity(1), 0), (entity(2), 0), (entity(3), 0)]);
        let resident = |e: Entity| match e.index_u32() {
            2 => Some((10..10, 0)),
            3 => Some((200..210, 1)),
            _ => None,
        };
        let batch = plan_owed(&mut pending, resident, 100, 65_535);
        assert!(batch.jobs.is_empty() && batch.waiting.is_empty());
        assert!(pending.is_empty());
    }

    #[test]
    fn deferred_work_advances_every_frame_and_retires_when_baked() {
        // Two workgroups per axis: at most four 128-quad pieces per dispatch.
        let mut pending = HashMap::from([(entity(1), 0), (entity(2), 0)]);
        let resident = |e: Entity| match e.index_u32() {
            1 => Some((0..600, 0)),
            2 => Some((1000..1010, 1)),
            _ => None,
        };
        let mut baked = Vec::new();
        for _frame in 0..4 {
            if pending.is_empty() {
                break;
            }
            let batch = plan_owed(&mut pending, resident, 4096, 2);
            assert!(!batch.jobs.is_empty() && batch.jobs.len() <= 4);
            assert!(batch.jobs.iter().all(|job| job[1] <= 128));
            for &[start, count, metadata, _] in &batch.jobs {
                baked.extend((start..start + count).map(|quad| (quad, metadata)));
            }
            commit(&mut pending, &batch);
        }
        assert!(pending.is_empty(), "the queue drains");
        baked.sort_unstable();
        let expected: Vec<_> = (0..600)
            .map(|quad| (quad, 0))
            .chain((1000..1010).map(|quad| (quad, 1)))
            .collect();
        assert_eq!(baked, expected, "every quad bakes exactly once");
    }

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
