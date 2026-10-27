//! World material compute (`render_world_material_passes`): each published compute's cells,
//! which the host owns for its bundle, and the pipeline that sets every cell once a frame to the
//! guest `step`'s result from the previous frame's cells. Materials read the current cells; the
//! guest never reads them back. A frame runs at most [`MAX_FRAME_CELLS`] cell updates across
//! every bundle; the computes past that wait, and the next frame starts with one of them.

use bevy::asset::uuid::Uuid;
use bevy::prelude::{Handle, Shader};
use bevy::render::{
    render_resource::{
        BindGroup, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType,
        Buffer, BufferBindingType, BufferDescriptor, BufferSize, BufferUsages,
        CachedComputePipelineId, ComputePipelineDescriptor, PipelineCache, ShaderStages,
    },
    renderer::{RenderDevice, RenderQueue},
};
use mod_render::world_material::{compute, publication::PublishedCompute};
use std::collections::HashMap;

/// Shader namespace of compute modules.
pub(crate) const COMPUTE_SHADERS: u64 = u64::from_le_bytes(*b"wmcompu\0");

/// Provisional: the most cell updates a frame runs across every bundle; computes past it wait
/// for a later frame's turn.
pub(crate) const MAX_FRAME_CELLS: u64 = 1 << 20;

/// The module of compute `program` ([`compute::WorldCompute::program`]).
pub(crate) fn shader(program: u128) -> Handle<Shader> {
    Handle::Uuid(
        Uuid::from_u64_pair(COMPUTE_SHADERS ^ (program >> 64) as u64, program as u64),
        std::marker::PhantomData,
    )
}

/// The layout of a compute's group: the frame uniform, the previous cells read-only and the
/// next cells the entry writes.
pub(crate) fn layout() -> BindGroupLayoutDescriptor {
    let storage = |binding, read_only| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: BufferSize::new(16),
        },
        count: None,
    };
    BindGroupLayoutDescriptor::new(
        "world material compute layout",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(compute::FRAME_UNIFORM_BYTES as u64),
                },
                count: None,
            },
            storage(1, true),
            storage(2, false),
        ],
    )
}

/// One compute's GPU resources.
pub(crate) struct ComputeEntry {
    cells: u32,
    params: [f32; 16],
    uniform: Buffer,
    /// The cells materials and the next step read.
    pub(crate) current: Buffer,
    next: Buffer,
    group: BindGroup,
    pipeline: CachedComputePipelineId,
}

impl ComputeEntry {
    /// Creates `published`'s zeroed cells, its group and its queued pipeline.
    pub(crate) fn new(
        device: &RenderDevice,
        cache: &PipelineCache,
        published: &PublishedCompute,
    ) -> Self {
        let bytes = u64::from(published.cells.max(1)) * 16;
        let buffer = |label| {
            device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let current = buffer("world material cells");
        let next = buffer("world material next cells");
        let uniform = device.create_buffer(&BufferDescriptor {
            label: Some("world material compute frame"),
            size: compute::FRAME_UNIFORM_BYTES as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(
            Some("world material compute"),
            &cache.get_bind_group_layout(&layout()),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: current.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: next.as_entire_binding(),
                },
            ],
        );
        let pipeline = cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("world material compute".into()),
            layout: vec![layout()],
            shader: shader(published.compute.program()),
            entry_point: Some(compute::ENTRY.into()),
            ..Default::default()
        });
        Self {
            cells: published.cells,
            params: published.params,
            uniform,
            current,
            next,
            group,
            pipeline,
        }
    }

    /// Takes `published`'s new parameters; its cells and pipeline stay.
    pub(crate) fn update(&mut self, published: &PublishedCompute) {
        self.params = published.params;
    }

    /// The compute's pipeline, which may have failed to compile.
    pub(crate) fn pipeline(&self) -> CachedComputePipelineId {
        self.pipeline
    }
}

/// Which of computes of `cells` cells each, in their order, run this frame within
/// [`MAX_FRAME_CELLS`]: starting at `cursor` and wrapping, each that fits spends its cells, and
/// one past what is left is skipped while later, smaller ones may still run. Returns which run
/// and the next frame's cursor: the first compute passed over that a frame could hold, or
/// `cursor` when none was. Each frame then starts with one the last left out, so every compute
/// that fits a frame on its own steps within a bounded number of frames.
pub(crate) fn within_frame(cells: &[u32], cursor: usize) -> (Vec<bool>, usize) {
    let mut fits = vec![false; cells.len()];
    if cells.is_empty() {
        return (fits, 0);
    }
    let start = cursor % cells.len();
    let mut budget = MAX_FRAME_CELLS;
    let mut next = None;
    for index in (start..cells.len()).chain(0..start) {
        let count = u64::from(cells[index]);
        if count <= budget {
            fits[index] = true;
            budget -= count;
        } else if next.is_none() && count <= MAX_FRAME_CELLS {
            next = Some(index);
        }
    }
    (fits, next.unwrap_or(start))
}

/// Writes every compute's frame uniform and encodes this frame's steps in `computes` order,
/// [`within_frame`] from `cursor`, which it advances for the next frame: each ready one
/// dispatches one invocation per cell into its next cells and copies them over its current
/// ones. Returns how many ran.
pub(crate) fn encode<'a>(
    computes: impl IntoIterator<Item = &'a ComputeEntry>,
    phase: f32,
    (queue, cache): (&RenderQueue, &PipelineCache),
    encoder: &mut wgpu::CommandEncoder,
    cursor: &mut usize,
) -> usize {
    let ready: Vec<_> = computes
        .into_iter()
        .filter_map(|entry| {
            let words = compute::frame_uniform(phase, entry.cells, &entry.params);
            queue.write_buffer(&entry.uniform, 0, bytemuck::cast_slice(&words));
            Some((entry, cache.get_compute_pipeline(entry.pipeline)?))
        })
        .collect();
    let cells: Vec<u32> = ready.iter().map(|(entry, _)| entry.cells).collect();
    let (fits, next) = within_frame(&cells, *cursor);
    *cursor = next;
    let mut ran = 0;
    for ((entry, pipeline), fits) in ready.into_iter().zip(fits) {
        if !fits {
            continue;
        }
        let cells = u64::from(entry.cells);
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("world material compute"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &*entry.group, &[]);
            pass.dispatch_workgroups(entry.cells.div_ceil(compute::WORKGROUP_SIZE), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&entry.next, 0, &entry.current, 0, cells * 16);
        ran += 1;
    }
    ran
}

/// Every published compute's resources, by key.
#[derive(Default)]
pub(crate) struct Computes {
    pub(crate) entries: HashMap<u64, ComputeEntry>,
    /// The key of each of the set's computes, by index; `None` before its resources exist.
    pub(crate) order: Vec<u64>,
}

impl Computes {
    /// Matches the published `computes`: creates new ones, updates kept ones' parameters and
    /// drops the rest.
    pub(crate) fn sync(
        &mut self,
        computes: &[PublishedCompute],
        device: &RenderDevice,
        cache: &PipelineCache,
    ) {
        self.entries
            .retain(|key, _| computes.iter().any(|published| published.key == *key));
        for published in computes {
            self.entries
                .entry(published.key)
                .and_modify(|entry| entry.update(published))
                .or_insert_with(|| ComputeEntry::new(device, cache, published));
        }
        self.order = computes.iter().map(|published| published.key).collect();
    }

    /// The resources of the set's compute `index`.
    pub(crate) fn get(&self, index: u16) -> Option<(u64, &ComputeEntry)> {
        let key = *self.order.get(usize::from(index))?;
        Some((key, self.entries.get(&key)?))
    }

    /// The entries in the set's order.
    pub(crate) fn ordered(&self) -> impl Iterator<Item = &ComputeEntry> {
        self.order.iter().filter_map(|key| self.entries.get(key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The showcase's compute steps, as its client part declares them.
    const SHOWCASE_STEPS: [&str; 2] = [
        include_str!("../../../../examples/experiences/material-fx/client/shaders/storm_step.wgsl"),
        include_str!("../../../../examples/experiences/material-fx/client/shaders/fire_step.wgsl"),
    ];

    /// Each showcase compute compiles through the pipeline cache as the renderer queues it: the
    /// entry and its layout queued in the frame the compute is published, before the module the
    /// main world registers reaches the cache. Waiting for the module is not a failure, and once
    /// it arrives the pipeline compiles.
    #[test]
    fn the_showcase_computes_compile_through_the_pipeline_cache_once_their_modules_arrive() {
        let Some((device, _queue, _adapter)) = crate::scene_sampling::tests::fixture() else {
            return;
        };
        bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
        let mut cache = PipelineCache::new(device.clone(), false);
        let failures = render_scene::world_material::WorldMaterialFailures::default();
        let published: Vec<PublishedCompute> = SHOWCASE_STEPS
            .iter()
            .enumerate()
            .map(|(key, source)| PublishedCompute {
                key: key as u64 + 1,
                compute: std::sync::Arc::new(compute::compose(source).unwrap()),
                cells: 64,
                params: [0.0; 16],
            })
            .collect();
        let entries: Vec<(u64, ComputeEntry)> = published
            .iter()
            .map(|published| (published.key, ComputeEntry::new(&device, &cache, published)))
            .collect();
        let pipelines = || entries.iter().map(|(key, entry)| (*key, entry.pipeline()));
        // Frames before the modules arrive.
        for _ in 0..3 {
            cache.process_queue();
            crate::world_material::record_compute_errors(&cache, pipelines(), &failures);
        }
        for published in &published {
            assert!(!failures.failed(published.key), "waiting is not failing");
            cache.set_shader(
                shader(published.compute.program()).id(),
                crate::shader_safety::from_wgsl(
                    published.compute.module(),
                    format!("src/world_material/compute_{}.wgsl", published.key),
                ),
            );
        }
        for (key, entry) in &entries {
            test_time::eventually("the compute pipeline compiles", || {
                cache.process_queue();
                crate::world_material::record_compute_errors(&cache, pipelines(), &failures);
                assert!(!failures.failed(*key), "compute {key} failed");
                matches!(
                    cache.get_compute_pipeline_state(entry.pipeline()),
                    bevy::render::render_resource::CachedPipelineState::Ok(_)
                )
            });
        }
    }

    #[test]
    fn a_frame_runs_computes_within_its_cell_budget() {
        let half = (MAX_FRAME_CELLS / 2) as u32;
        let (fits, next) = within_frame(&[half, half, 1, half, 0], 0);
        assert_eq!(fits, [true, true, false, false, true]);
        assert_eq!(next, 2, "the next frame starts with the first one left out");
        let (fits, next) = within_frame(&[u32::MAX, 16, half], 0);
        assert_eq!(
            fits,
            [false, true, true],
            "an oversized one waits; smaller ones still run"
        );
        assert_eq!(next, 0, "one no frame can hold never takes the lead");
        assert_eq!(within_frame(&[], 3), (Vec::new(), 0));
    }

    #[test]
    fn every_compute_steps_when_more_than_a_frame_of_bundles_need_cells() {
        // Five full bundles, each a quarter of a frame's cells.
        let quarter = (MAX_FRAME_CELLS / 4) as u32;
        let cells = [quarter; 5];
        let mut stepped = [0_u32; 5];
        let mut cursor = 0;
        for _ in 0..10 {
            let (fits, next) = within_frame(&cells, cursor);
            let spent: u64 = cells
                .iter()
                .zip(&fits)
                .filter(|(_, fits)| **fits)
                .map(|(cells, _)| u64::from(*cells))
                .sum();
            assert!(spent <= MAX_FRAME_CELLS, "a frame stays within its budget");
            for (count, fits) in stepped.iter_mut().zip(fits) {
                *count += u32::from(fits);
            }
            cursor = next;
        }
        assert!(
            stepped.iter().all(|count| *count >= 7),
            "every compute steps in turn: {stepped:?}"
        );
    }
}
