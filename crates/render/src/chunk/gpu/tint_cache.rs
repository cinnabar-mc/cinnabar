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
use crate::queue_writes::ProfiledQueue as _;
use bevy::{
    prelude::{Entity, FromWorld, Res, ResMut, Resource, World},
    render::{
        render_resource::{
            BindGroup, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBinding,
            BufferBindingType, BufferDescriptor, BufferId, BufferUsages, CachedComputePipelineId,
            ComputePassDescriptor, ComputePipelineDescriptor, PipelineCache, ShaderStages,
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
    /// Persistent job buffer: a count header, then up to `job_capacity` jobs.
    jobs: Buffer,
    job_capacity: u32,
    /// Group 0 of the bake over the tables it reads, by their identities; rebuilt only when
    /// one of them is replaced.
    source_group: Option<([BufferId; 6], BindGroup)>,
    /// Group 1 over `jobs` and the cache; rebuilt only when the job buffer grows.
    output_group: Option<BindGroup>,
}

/// The packed-chunk tables a bake reads, in [`plan::SOURCE_STORAGE_BINDINGS`] order, then the
/// biome query tables. Every one is replaced only together with a buffer or table identity the
/// terrain bind groups track, so their rebuild frames cover any replacement.
pub(in crate::chunk) struct BakeSources<'a> {
    pub(in crate::chunk) quads: &'a Buffer,
    pub(in crate::chunk) origins: &'a Buffer,
    pub(in crate::chunk) materials: &'a Buffer,
    pub(in crate::chunk) biome_records: &'a Buffer,
    pub(in crate::chunk) biome_tints: &'a Buffer,
    pub(in crate::chunk) query_tables: &'a Buffer,
}

impl BakeSources<'_> {
    fn storage(&self) -> [&Buffer; 5] {
        [
            self.quads,
            self.origins,
            self.materials,
            self.biome_records,
            self.biome_tints,
        ]
    }

    fn ids(&self) -> [BufferId; 6] {
        let [a, b, c, d, e] = self.storage().map(Buffer::id);
        [a, b, c, d, e, self.query_tables.id()]
    }
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
    /// Jobs the last bake owed beyond the job buffer, which the next preparation grows toward.
    wanted_jobs: usize,
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

    /// Frees the cache for required terrain storage, or after the device refused its bake
    /// bindings. It is not recreated until the arena's quad capacity or the tint tables change,
    /// and bind groups fall back to the never-valid alias. Returns whether there was a cache.
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
        let buffer = create(device, "terrain tint cache", entries * ENTRY_BYTES);
        let jobs = buffer.as_ref().and_then(|_| {
            create(
                device,
                "terrain tint bake jobs",
                plan::job_buffer_bytes(plan::INITIAL_JOB_CAPACITY),
            )
        });
        self.cache = buffer.zip(jobs).map(|(buffer, jobs)| TintCache {
            buffer,
            entries,
            quad_capacity,
            inputs,
            jobs,
            job_capacity: plan::INITIAL_JOB_CAPACITY,
            source_group: None,
            output_group: None,
        });
        self.failed = self.cache.is_none().then_some((quad_capacity, inputs));
        self.pending.clear();
        self.bake_all = self.cache.is_some();
        self.wanted_jobs = 0;
    }

    /// On frames that rebuild the terrain bind groups (`rebuilding`), grows the job buffer
    /// toward what bakes owed and (re)builds the bake's bind groups where a buffer they bind was
    /// replaced; other frames create nothing, so descriptors are only allocated where the
    /// renderer allocates them anyway, and owed growth waits. Bakes themselves create no buffers
    /// or bind groups. A job buffer that cannot grow keeps its capacity, and bakes spread over
    /// more frames.
    pub(in crate::chunk) fn prepare_bake(
        &mut self,
        device: &RenderDevice,
        rebuilding: bool,
        layouts: (&BindGroupLayout, &BindGroupLayout),
        sources: &BakeSources<'_>,
    ) {
        self.prepare_bake_with(
            device,
            rebuilding,
            layouts,
            sources,
            |label, layout, entries| {
                crate::gpu_allocation::checked(device, label, 0, || {
                    device.create_bind_group(label, layout, entries)
                })
            },
        );
    }

    /// [`Self::prepare_bake`] with `create_group` making each bind group or reporting that the
    /// device refused it. A refused source group, or output group of a new cache, releases the
    /// cache so the opaque group binds the never-valid alias. A refused output group for a grown
    /// job buffer keeps the current buffer and group and leaves the growth owed.
    fn prepare_bake_with(
        &mut self,
        device: &RenderDevice,
        rebuilding: bool,
        layouts: (&BindGroupLayout, &BindGroupLayout),
        sources: &BakeSources<'_>,
        mut create_group: impl FnMut(
            &'static str,
            &BindGroupLayout,
            &[BindGroupEntry],
        ) -> Option<BindGroup>,
    ) {
        if !rebuilding {
            return;
        }
        let wanted = std::mem::take(&mut self.wanted_jobs);
        let Some(cache) = &mut self.cache else {
            return;
        };
        let capacity = plan::grown_job_capacity(cache.job_capacity, wanted, &device.limits());
        let mut grown = None;
        if capacity > cache.job_capacity && cache.output_group.is_some() {
            grown = create(
                device,
                "terrain tint bake jobs",
                plan::job_buffer_bytes(capacity),
            )
            .and_then(|jobs| {
                create_group(
                    "terrain tint bake output",
                    layouts.1,
                    &output_entries(&jobs, &cache.buffer),
                )
                .map(|group| (jobs, group))
            });
        }
        let growth_owed = capacity > cache.job_capacity && grown.is_none();
        let ids = sources.ids();
        let source_group = if cache
            .source_group
            .as_ref()
            .is_none_or(|(bound, _)| *bound != ids)
        {
            let entries: Vec<_> = sources
                .storage()
                .map(|buffer| buffer.as_entire_binding())
                .into_iter()
                .zip(plan::SOURCE_STORAGE_BINDINGS)
                .chain([(
                    sources.query_tables.as_entire_binding(),
                    crate::material_shader::BIOME_QUERY_TABLES_BINDING,
                )])
                .map(|(resource, binding)| BindGroupEntry { binding, resource })
                .collect();
            match create_group("terrain tint bake sources", layouts.0, &entries) {
                Some(group) => Some(group),
                None => {
                    self.release_for_required_storage();
                    return;
                }
            }
        } else {
            None
        };
        let output_group = match (&grown, &cache.output_group) {
            (None, None) => {
                match create_group(
                    "terrain tint bake output",
                    layouts.1,
                    &output_entries(&cache.jobs, &cache.buffer),
                ) {
                    Some(group) => Some(group),
                    None => {
                        self.release_for_required_storage();
                        return;
                    }
                }
            }
            _ => None,
        };
        if let Some(group) = source_group {
            cache.source_group = Some((ids, group));
        }
        if let Some(group) = output_group {
            cache.output_group = Some(group);
        }
        if let Some((jobs, group)) = grown {
            cache.jobs = jobs;
            cache.job_capacity = capacity;
            cache.output_group = Some(group);
        }
        if growth_owed {
            self.wanted_jobs = wanted;
        }
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
        self.cache
            .as_ref()
            .map_or(0, |cache| cache.buffer.size() + cache.jobs.size())
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
        let Some((entries, job_capacity)) = self
            .cache
            .as_ref()
            .map(|cache| (cache.entries, cache.job_capacity))
        else {
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
            plan::reach(limit).min(job_capacity as usize),
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
/// the rest bake in entity order, in pieces within `limit` workgroups per axis, up to
/// `max_jobs` pieces from the quads earlier frames already baked, and the remainder waits.
fn plan_owed(
    pending: &mut HashMap<Entity, u32>,
    resident: impl Fn(Entity) -> Option<(std::ops::Range<u32>, u32)>,
    entries: u64,
    limit: u32,
    max_jobs: usize,
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
    for (entity, done) in owed {
        let Some(job) = remaining(entity, done) else {
            continue;
        };
        let mut baked = 0;
        for piece in plan::split(job, limit) {
            if batch.jobs.len() < max_jobs {
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

/// Group 1 of the bake: `jobs`, then the `cache` it writes.
fn output_entries<'a>(jobs: &'a Buffer, cache: &'a Buffer) -> [BindGroupEntry<'a>; 2] {
    [
        BindGroupEntry {
            binding: 0,
            resource: jobs.as_entire_binding(),
        },
        BindGroupEntry {
            binding: 1,
            resource: cache.as_entire_binding(),
        },
    ]
}

/// A zeroed storage buffer of `bytes`, or `None` when the device refuses the allocation.
fn create(device: &RenderDevice, label: &'static str, bytes: u64) -> Option<Buffer> {
    crate::gpu_allocation::buffer(
        device,
        &BufferDescriptor {
            label: Some(label),
            size: bytes,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        },
    )
}

/// The bake kernel, queued only when the opaque pipelines read the cache.
#[derive(Resource)]
pub(in crate::chunk) struct TintBakePipeline {
    id: Option<CachedComputePipelineId>,
    sources: BindGroupLayoutDescriptor,
    output: BindGroupLayoutDescriptor,
}

impl TintBakePipeline {
    /// The kernel's group 0 and group 1 layouts.
    pub(in crate::chunk) fn layouts(
        &self,
        cache: &PipelineCache,
    ) -> (BindGroupLayout, BindGroupLayout) {
        (
            cache.get_bind_group_layout(&self.sources),
            cache.get_bind_group_layout(&self.output),
        )
    }
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

/// Bakes the owed entries one dispatch and the job buffer reach ahead of every camera pass,
/// and clears the rest and any whose bake cannot be recorded, so the vertex stage computes them
/// until they bake. It only writes jobs into the persistent buffer and records the dispatch.
pub(in crate::chunk) fn record_tint_bake(
    arena: Option<ResMut<crate::chunk::ChunkGpuArena>>,
    pipeline: Option<Res<TintBakePipeline>>,
    cache: Res<PipelineCache>,
    gpu: (Res<RenderDevice>, Res<RenderQueue>),
    mut context: RenderContext,
) {
    let Some(mut arena) = arena else {
        return;
    };
    let arena = &mut *arena;
    let (device, queue) = gpu;
    let limit = device.limits().max_compute_workgroups_per_dimension;
    let batch = arena.tint_cache.plan_batch(&arena.allocations, limit);
    let state = &mut arena.tint_cache;
    let Some(tint) = &state.cache else {
        return;
    };
    if batch.jobs.is_empty() {
        // Every owed allocation was dropped as departed, empty or beyond the cache.
        return;
    }
    let encoder = context.command_encoder();
    plan::clear(encoder, &tint.buffer, &batch.waiting);
    let owed = batch.jobs.len() + batch.waiting.len();
    let kernel = pipeline
        .as_deref()
        .and_then(|pipeline| pipeline.id)
        .and_then(|id| cache.get_compute_pipeline(id));
    let (Some(kernel), Some((_, source_group)), Some(output_group)) =
        (kernel, &tint.source_group, &tint.output_group)
    else {
        // The queue keeps the work; its entries stay cleared until a bake can be recorded.
        plan::clear(encoder, &tint.buffer, &batch.jobs);
        return;
    };
    let words = plan::job_words(&batch.jobs);
    let written = crate::gpu_allocation::checked(
        &device,
        "terrain tint bake jobs",
        (words.len() * 4) as u64,
        || queue.profiled_write_buffer(&tint.jobs, 0, bytemuck::cast_slice(&words)),
    );
    if written.is_none() {
        // The buffer still holds an earlier batch: bake nothing, credit no progress, and keep
        // this batch's entries cleared until a write succeeds.
        plan::clear(encoder, &tint.buffer, &batch.jobs);
        if owed > tint.job_capacity as usize {
            state.wanted_jobs = owed;
        }
        return;
    }
    {
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("terrain tint bake"),
            timestamp_writes: None,
        });
        pass.set_pipeline(kernel);
        pass.set_bind_group(0, source_group, &[]);
        pass.set_bind_group(1, output_group, &[]);
        plan::dispatch(&mut pass, &batch.jobs, limit);
    }
    if owed > tint.job_capacity as usize {
        state.wanted_jobs = owed;
    }
    state.commit(&batch);
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

    /// A noop device with every table a bake binds, and a cache prepared over them.
    struct BakeFixture {
        device: RenderDevice,
        layouts: (BindGroupLayout, BindGroupLayout),
        tables: [Buffer; 5],
        query_tables: Buffer,
    }

    impl BakeFixture {
        fn new() -> Self {
            let (device, _queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
            let device = RenderDevice::from(device);
            let tables = ["quads", "origins", "materials", "records", "tints"]
                .map(|label| create(&device, label, 64).unwrap());
            let query_tables = device.create_buffer(&BufferDescriptor {
                label: Some("query tables"),
                size: 64,
                usage: BufferUsages::UNIFORM,
                mapped_at_creation: false,
            });
            let layouts = (
                device.create_bind_group_layout(None, &plan::source_layout_entries()),
                device.create_bind_group_layout(None, &plan::output_layout_entries()),
            );
            Self {
                device,
                layouts,
                tables,
                query_tables,
            }
        }

        fn state(&self) -> TintCacheState {
            let mut state = TintCacheState::default();
            state.prepare(
                &self.device,
                64,
                TintInputs {
                    materials: self.tables[2].id(),
                    biome_tints: self.tables[4].id(),
                    biome_tint_table: ChunkBiomeTintResourceIdentity {
                        pointer: 1,
                        table: meshing::ChunkBiomeTintIdentity::new(1, 1),
                    },
                },
            );
            assert!(state.cache.is_some());
            state
        }

        /// The fixture's tables, with `quads` in place of its own when given.
        fn sources<'a>(&'a self, quads: Option<&'a Buffer>) -> BakeSources<'a> {
            let [own_quads, origins, materials, biome_records, biome_tints] = &self.tables;
            BakeSources {
                quads: quads.unwrap_or(own_quads),
                origins,
                materials,
                biome_records,
                biome_tints,
                query_tables: &self.query_tables,
            }
        }

        fn prepare(&self, state: &mut TintCacheState, rebuilding: bool, quads: Option<&Buffer>) {
            state.prepare_bake(
                &self.device,
                rebuilding,
                (&self.layouts.0, &self.layouts.1),
                &self.sources(quads),
            );
        }

        /// Prepares with the device refusing the bind group labelled `refused`.
        fn prepare_refusing(&self, state: &mut TintCacheState, refused: &str) {
            state.prepare_bake_with(
                &self.device,
                true,
                (&self.layouts.0, &self.layouts.1),
                &self.sources(None),
                |label, layout, entries| {
                    (label != refused)
                        .then(|| self.device.create_bind_group(label, layout, entries))
                },
            );
        }
    }

    fn identity(
        state: &TintCacheState,
    ) -> (
        BufferId,
        bevy::render::render_resource::BindGroupId,
        bevy::render::render_resource::BindGroupId,
    ) {
        let cache = state.cache.as_ref().unwrap();
        (
            cache.jobs.id(),
            cache.source_group.as_ref().unwrap().1.id(),
            cache.output_group.as_ref().unwrap().id(),
        )
    }

    #[test]
    fn bakes_reuse_buffers_and_bind_groups_until_one_is_replaced() {
        let fixture = BakeFixture::new();
        let mut state = fixture.state();
        fixture.prepare(&mut state, true, None);
        let first = identity(&state);
        fixture.prepare(&mut state, true, None);
        assert_eq!(identity(&state), first, "unchanged inputs create nothing");
        state.wanted_jobs = plan::INITIAL_JOB_CAPACITY as usize + 1;
        fixture.prepare(&mut state, false, None);
        assert_eq!(
            identity(&state),
            first,
            "stable terrain groups defer growth"
        );
        assert_eq!(state.wanted_jobs, plan::INITIAL_JOB_CAPACITY as usize + 1);
        fixture.prepare(&mut state, true, None);
        let grown = identity(&state);
        assert_ne!(grown.0, first.0, "an overflowing bake grows the job buffer");
        assert_eq!(grown.1, first.1, "the source tables were not replaced");
        assert_ne!(grown.2, first.2);
        let moved = create(&fixture.device, "migrated quads", 64).unwrap();
        fixture.prepare(&mut state, true, Some(&moved));
        let rebound = identity(&state);
        assert_eq!(rebound.0, grown.0);
        assert_ne!(rebound.1, grown.1, "a replaced table rebinds the sources");
    }

    #[test]
    fn refused_bake_bindings_release_the_cache_or_keep_the_current_jobs() {
        let fixture = BakeFixture::new();
        for refused in ["terrain tint bake sources", "terrain tint bake output"] {
            let mut state = fixture.state();
            state.queue_bake(entity(1));
            fixture.prepare_refusing(&mut state, refused);
            assert!(
                state.cache.is_none(),
                "{refused}: a new cache without bindings is released"
            );
            assert!(!state.has_work() && state.id().is_none());
            fixture.prepare(&mut state, true, None);
            assert!(
                state.cache.is_none(),
                "{refused}: not recreated for the same inputs"
            );
        }
        let mut state = fixture.state();
        fixture.prepare(&mut state, true, None);
        let before = identity(&state);
        let wanted = plan::INITIAL_JOB_CAPACITY as usize + 1;
        state.wanted_jobs = wanted;
        fixture.prepare_refusing(&mut state, "terrain tint bake output");
        assert_eq!(
            identity(&state),
            before,
            "a refused growth keeps the paired buffer and group"
        );
        assert_eq!(
            state.cache.as_ref().unwrap().job_capacity,
            plan::INITIAL_JOB_CAPACITY
        );
        assert_eq!(state.wanted_jobs, wanted, "the growth stays owed");
    }

    #[test]
    fn owed_work_drops_departed_empty_and_uncached_allocations_without_a_dispatch() {
        let mut pending = HashMap::from([(entity(1), 0), (entity(2), 0), (entity(3), 0)]);
        let resident = |e: Entity| match e.index_u32() {
            2 => Some((10..10, 0)),
            3 => Some((200..210, 1)),
            _ => None,
        };
        let batch = plan_owed(&mut pending, resident, 100, 65_535, 256);
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
            let batch = plan_owed(&mut pending, resident, 4096, 2, plan::reach(2));
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
