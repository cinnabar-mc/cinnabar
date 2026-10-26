//! Compute expansion of a pack's geometry shader, streamed through retained scratch buffers.

use super::draws::{DrawKind, GeometryDraw};
use super::geometry::{CONSTANT_STREAM_WORDS, TerrainLayout, vertex_buffer_layouts};
use super::passes::ViewState;
use super::targets::Groups;
use shaderpack::{CompiledStage, GeometryEmulation, ProgramReflection, ScalarType, ValueType};
use std::borrow::Cow;
use wgpu::util::StagingBelt;

/// Allocation owner used to report and test recoverable expansion-storage failures.
pub(crate) const SCRATCH_ALLOCATION: &str = "shader pack geometry expansion";

/// One existing triangle stream, including indexed terrain and non-indexed actor streams.
pub(crate) struct Input<'a> {
    pub vertices: &'a wgpu::Buffer,
    pub indices: Option<&'a wgpu::Buffer>,
    /// CPU indices for effect meshes whose vertex buffer can span several storage windows.
    pub index_values: Option<&'a [u32]>,
    pub origins: Option<&'a wgpu::Buffer>,
    pub first: u32,
    pub count: u32,
    pub base_vertex: i32,
    pub instance: u32,
}

/// Immutable pipelines compiled when the pack loads, specialized to one host vertex layout.
pub(crate) struct Pipelines {
    vertex: wgpu::ComputePipeline,
    geometry: wgpu::ComputePipeline,
    compute_layout: wgpu::BindGroupLayout,
    pub raster_layout: wgpu::BindGroupLayout,
    pub raster_pipeline_layout: wgpu::PipelineLayout,
    pub raster_vertex: wgpu::ShaderModule,
    pub contract: GeometryEmulation,
    vertex_stride: u64,
}

/// Retained per-view expansion storage. Batches stream through it without dropping primitives.
pub(crate) struct Scratch {
    device: wgpu::Device,
    input: wgpu::Buffer,
    output: wgpu::Buffer,
    pub(super) indices: wgpu::Buffer,
    params: wgpu::Buffer,
    dummy: wgpu::Buffer,
    pub(super) indirect: wgpu::Buffer,
    belt: StagingBelt,
    pub capacity: u32,
}

/// Returns the GLSL scalar/vector constructor for a reflected attribute.
fn glsl_type(ty: ValueType) -> Result<String, String> {
    let (scalar_type, width) = match ty {
        ValueType::Scalar(s) => (s, 1),
        ValueType::Vector { scalar, size } => (scalar, size),
        _ => return Err("geometry attribute matrices cannot be decoded".into()),
    };
    let (scalar, prefix) = match scalar_type {
        ScalarType::Float => ("float", "vec"),
        ScalarType::Int => ("int", "ivec"),
        ScalarType::Uint => ("uint", "uvec"),
        _ => return Err("geometry vertex attribute needs 32-bit numeric components".into()),
    };
    Ok(if width == 1 {
        scalar.into()
    } else {
        format!("{prefix}{}", width)
    })
}

/// Decodes a vertex-format component with the same normalized/integer conversion as vertex fetch.
fn component(
    format: wgpu::VertexFormat,
    word: &dyn Fn(u32) -> String,
    index: u32,
) -> Result<String, String> {
    use wgpu::VertexFormat as F;
    let components = match format {
        F::Float32 | F::Uint32 | F::Sint32 => 1,
        F::Float32x2
        | F::Uint32x2
        | F::Sint32x2
        | F::Unorm16x2
        | F::Float16x2
        | F::Sint16x2
        | F::Uint16x2 => 2,
        F::Float32x3 | F::Uint32x3 | F::Sint32x3 => 3,
        F::Float32x4
        | F::Uint32x4
        | F::Sint32x4
        | F::Unorm8x4
        | F::Snorm8x4
        | F::Sint8x4
        | F::Uint8x4
        | F::Uint16x4 => 4,
        _ => {
            return Err(format!(
                "geometry vertex format {format:?} cannot be decoded"
            ));
        }
    };
    if index >= components {
        return Ok(if index == 3 { "1" } else { "0" }.into());
    }
    Ok(match format {
        F::Float32 | F::Float32x2 | F::Float32x3 | F::Float32x4 => {
            format!("uintBitsToFloat({})", word(index))
        }
        F::Uint32 | F::Uint32x2 | F::Uint32x3 | F::Uint32x4 => word(index),
        F::Sint32 | F::Sint32x2 | F::Sint32x3 | F::Sint32x4 => format!("int({})", word(index)),
        F::Unorm8x4 => format!("unpackUnorm4x8({})[{index}]", word(0)),
        F::Snorm8x4 => format!("unpackSnorm4x8({})[{index}]", word(0)),
        F::Unorm16x2 => format!("unpackUnorm2x16({})[{index}]", word(0)),
        F::Float16x2 => format!("unpackHalf2x16({})[{index}]", word(0)),
        F::Sint8x4 => format!("bitfieldExtract(int({}),{},8)", word(0), index * 8),
        F::Sint16x2 => format!("bitfieldExtract(int({}),{},16)", word(0), index * 16),
        F::Uint8x4 => format!("bitfieldExtract({},{},8)", word(0), index * 8),
        F::Uint16x2 | F::Uint16x4 => format!(
            "bitfieldExtract({},{},16)",
            word(index / 2),
            (index % 2) * 16
        ),
        _ => {
            return Err(format!(
                "geometry vertex format {format:?} cannot be decoded"
            ));
        }
    })
}

/// Generates assignments from the existing host streams; no world data is copied on the CPU.
fn attribute_loads(
    kind: DrawKind,
    terrain: TerrainLayout,
    reflection: &ProgramReflection,
) -> Result<(Vec<(u32, String)>, u64), String> {
    let layouts = if kind.actor() {
        super::actors::layout::vertex_layouts(&reflection.vertex_inputs)
    } else {
        vertex_buffer_layouts(
            &reflection.vertex_inputs,
            &if kind.category().is_some() {
                terrain
            } else {
                TerrainLayout::WIDE
            },
        )
    }
    .map_err(|error| error.to_string())?;
    let mut loads = Vec::new();
    for (stream, layout) in layouts.iter().enumerate() {
        for attribute in &layout.attributes {
            let input = reflection
                .vertex_inputs
                .iter()
                .find(|input| input.location == attribute.shader_location)
                .ok_or("geometry attribute has no reflected input")?;
            let word = |i: u32| {
                let offset = attribute.offset / 4 + u64::from(i);
                match stream {
                    0 => format!(
                        "cb_vertices[uint(cb_vertex_id)*{}u+{offset}u-cb_params[6]]",
                        layout.array_stride / 4
                    ),
                    1 => format!("{}u", CONSTANT_STREAM_WORDS[offset as usize]),
                    _ => format!(
                        "cb_origins[uint(cb_instance_id)*{}u+{offset}u-cb_params[7]]",
                        layout.array_stride / 4
                    ),
                }
            };
            let values = (0..match input.ty {
                ValueType::Scalar(_) => 1,
                ValueType::Vector { size, .. } => u32::from(size),
                _ => 0,
            })
                .map(|i| component(attribute.format, &word, i))
                .collect::<Result<Vec<_>, _>>()?;
            loads.push((
                input.location,
                format!("{}({})", glsl_type(input.ty)?, values.join(",")),
            ));
        }
    }
    Ok((
        loads,
        layouts
            .first()
            .map_or(4, |layout| layout.array_stride.max(4)),
    ))
}

/// Creates a storage layout with only the bindings used by each emulated stage.
fn storage_layout(
    device: &wgpu::Device,
    bindings: &[(u32, bool)],
    stages: wgpu::ShaderStages,
) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("geometry shader storage"),
        entries: &bindings
            .iter()
            .map(|&(binding, read_only)| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: stages,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect::<Vec<_>>(),
    })
}

/// Compiles a vertex/geometry pair once and creates the raster reader's pipeline layout.
#[expect(
    clippy::too_many_arguments,
    reason = "the existing program stages, bindings, and host vertex layout"
)]
pub(crate) fn pipelines(
    device: &wgpu::Device,
    kind: DrawKind,
    terrain: TerrainLayout,
    layouts: &[Option<wgpu::BindGroupLayout>; 4],
    reflection: &ProgramReflection,
    vertex: &CompiledStage,
    geometry: &CompiledStage,
    contract: &GeometryEmulation,
) -> Result<Pipelines, String> {
    let (loads, vertex_stride) = attribute_loads(kind, terrain, reflection)?;
    let bytes = (u64::from(contract.max_vertices) * u64::from(contract.output_words))
        .max(u64::from(contract.input_words) * 3)
        .max(u64::from(contract.max_vertices - 2) * 3)
        * 4;
    let bytes = bytes
        .max(vertex_stride * 4 + u64::from(device.limits().min_storage_buffer_offset_alignment));
    if bytes > device.limits().max_storage_buffer_binding_size
        || device.limits().max_storage_buffers_per_shader_stage < 5
    {
        return Err("the device cannot hold one emitted geometry primitive".into());
    }
    let vertex =
        shaderpack::geometry_vertex_compute(vertex, contract, &loads).map_err(|errors| {
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        })?;
    let vertex_layout = storage_layout(
        device,
        &[(0, true), (1, true), (2, true), (3, false), (6, true)],
        wgpu::ShaderStages::COMPUTE,
    );
    let compute_layout = storage_layout(
        device,
        &[(3, true), (4, false), (5, false), (6, true)],
        wgpu::ShaderStages::COMPUTE,
    );
    let raster_layout = storage_layout(device, &[(4, true)], wgpu::ShaderStages::VERTEX);
    let pipeline_layout = |storage: &wgpu::BindGroupLayout| {
        let mut groups = layouts
            .iter()
            .take(3)
            .map(Option::as_ref)
            .collect::<Vec<_>>();
        groups.push(Some(storage));
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("emulated geometry shader"),
            bind_group_layouts: &groups,
            immediate_size: 0,
        })
    };
    let pipeline = |stage: &CompiledStage, layout: &wgpu::BindGroupLayout| {
        let module = super::pipelines::pack_shader_module(
            device,
            wgpu::ShaderModuleDescriptor {
                label: Some("geometry shader compute"),
                source: wgpu::ShaderSource::Naga(Cow::Owned(stage.module.clone())),
            },
        );
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("geometry shader prepass"),
            layout: Some(&pipeline_layout(layout)),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    let raster_pipeline_layout = pipeline_layout(&raster_layout);
    let raster_vertex = super::pipelines::pack_shader_module(
        device,
        wgpu::ShaderModuleDescriptor {
            label: Some("geometry shader vertex reader"),
            source: wgpu::ShaderSource::Naga(Cow::Owned(contract.vertex.module.clone())),
        },
    );
    Ok(Pipelines {
        vertex: pipeline(&vertex, &vertex_layout),
        geometry: pipeline(geometry, &compute_layout),
        compute_layout,
        raster_layout,
        raster_pipeline_layout,
        raster_vertex,
        contract: contract.clone(),
        vertex_stride,
    })
}

impl Scratch {
    /// Sizes storage from the loaded contracts and device limits, then reuses it for every batch.
    pub(crate) fn new(device: &wgpu::Device, draws: &[GeometryDraw]) -> Option<Self> {
        let contracts = draws
            .iter()
            .filter_map(|draw| draw.emulation.as_ref().map(|p| &p.contract))
            .collect::<Vec<_>>();
        if contracts.is_empty() {
            return None;
        }
        let input_words = contracts.iter().map(|c| c.input_words * 3).max()?;
        let output_words = contracts
            .iter()
            .map(|c| c.output_words * c.max_vertices)
            .max()?;
        let index_words = contracts.iter().map(|c| (c.max_vertices - 2) * 3).max()?;
        // Effect meshes append fresh triangles or quads, so each triangle spans at most four vertices.
        let source_words = draws
            .iter()
            .filter_map(|draw| draw.emulation.as_ref())
            .map(|p| p.vertex_stride)
            .max()?;
        let max_words = u64::from(input_words.max(output_words).max(index_words)).max(source_words);
        let available = device.limits().max_storage_buffer_binding_size
            - u64::from(device.limits().min_storage_buffer_offset_alignment);
        let capacity = (available / 4 / max_words).clamp(1, 1024) as u32;
        let buffer = |label, size, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let storage = wgpu::BufferUsages::STORAGE;
        Some(Self {
            device: device.clone(),
            input: buffer(
                "geometry vertex results",
                u64::from(capacity) * u64::from(input_words) * 4,
                storage,
            ),
            output: buffer(
                "geometry emitted vertices",
                u64::from(capacity) * u64::from(output_words) * 4,
                storage,
            ),
            indices: buffer(
                "geometry emitted indices",
                u64::from(capacity) * u64::from(index_words) * 4,
                storage | wgpu::BufferUsages::INDEX,
            ),
            params: buffer(
                "geometry batch parameters",
                32,
                storage | wgpu::BufferUsages::COPY_DST,
            ),
            dummy: buffer("geometry unused input", 16, storage),
            indirect: buffer(
                "geometry indirect draw",
                20,
                wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
            ),
            belt: StagingBelt::new(device.clone(), 64 * 1024),
            capacity,
        })
    }

    /// Recovers staging memory whose previous frame has completed.
    pub(crate) fn begin(&mut self) {
        self.belt.recall();
    }
    /// Unmaps this frame's staging chunks before command submission.
    pub(crate) fn finish(&mut self) {
        self.belt.finish();
    }

    /// Records parameter copies followed by both compute passes; emitted buffers are consumed next.
    #[expect(
        clippy::too_many_arguments,
        reason = "one streamed batch and its existing host bindings"
    )]
    pub(super) fn expand(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        pipelines: &Pipelines,
        groups: &Groups,
        input: &Input<'_>,
        first: u32,
        count: u32,
        between: &mut dyn FnMut(&mut wgpu::CommandEncoder),
    ) -> wgpu::BindGroup {
        let alignment = u64::from(device.limits().min_storage_buffer_offset_alignment);
        let first_element = u64::from(input.first) + u64::from(first) * 3;
        let end_element = first_element + u64::from(count) * 3;
        let elements = input
            .indices
            .map(|buffer| storage_window(buffer, first_element * 4..end_element * 4, alignment));
        let vertex_range = if let Some(indices) = input.index_values {
            let indices = &indices[first_element as usize..end_element as usize];
            let min = i64::from(*indices.iter().min().unwrap()) + i64::from(input.base_vertex);
            let max = i64::from(*indices.iter().max().unwrap()) + i64::from(input.base_vertex);
            Some(min as u64 * pipelines.vertex_stride..(max as u64 + 1) * pipelines.vertex_stride)
        } else if input.indices.is_none() {
            let base = i64::from(input.base_vertex);
            Some(
                (first_element as i64 + base) as u64 * pipelines.vertex_stride
                    ..(end_element as i64 + base) as u64 * pipelines.vertex_stride,
            )
        } else {
            None // Indexed terrain vertices already live in storage-sized pages.
        };
        let vertices = vertex_range.map_or_else(
            || input.vertices.as_entire_buffer_binding(),
            |range| storage_window(input.vertices, range, alignment),
        );
        let origins = input.origins.map(|buffer| {
            let stride = super::geometry::DRAW_ORIGIN_BYTES;
            let start = u64::from(input.instance) * stride;
            storage_window(buffer, start..start + stride, alignment)
        });
        let params = [
            (first_element - elements.as_ref().map_or(0, |binding| binding.offset / 4)) as u32,
            u32::from(input.indices.is_some()),
            input.base_vertex as u32,
            input.instance,
            first,
            count,
            (vertices.offset / 4) as u32,
            origins
                .as_ref()
                .map_or(0, |binding| (binding.offset / 4) as u32),
        ];
        self.belt
            .write_buffer(
                encoder,
                &self.params,
                0,
                std::num::NonZeroU64::new(32).unwrap(),
            )
            .copy_from_slice(bytemuck::cast_slice(&params));
        let indices = count * (pipelines.contract.max_vertices - 2) * 3;
        let indirect = [indices, 1, 0, 0, 0];
        self.belt
            .write_buffer(
                encoder,
                &self.indirect,
                0,
                std::num::NonZeroU64::new(20).unwrap(),
            )
            .copy_from_slice(bytemuck::cast_slice(&indirect));
        let group = |layout: &wgpu::BindGroupLayout, buffers: &[(u32, wgpu::BufferBinding<'_>)]| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("geometry batch"),
                layout,
                entries: &buffers
                    .iter()
                    .map(|(binding, buffer)| wgpu::BindGroupEntry {
                        binding: *binding,
                        resource: wgpu::BindingResource::Buffer(buffer.clone()),
                    })
                    .collect::<Vec<_>>(),
            })
        };
        let vertex = group(
            &pipelines.vertex.get_bind_group_layout(3),
            &[
                (0, vertices),
                (
                    1,
                    elements.unwrap_or_else(|| self.dummy.as_entire_buffer_binding()),
                ),
                (
                    2,
                    origins.unwrap_or_else(|| self.dummy.as_entire_buffer_binding()),
                ),
                (3, self.input.as_entire_buffer_binding()),
                (6, self.params.as_entire_buffer_binding()),
            ],
        );
        let geometry = group(
            &pipelines.compute_layout,
            &[
                (3, self.input.as_entire_buffer_binding()),
                (4, self.output.as_entire_buffer_binding()),
                (5, self.indices.as_entire_buffer_binding()),
                (6, self.params.as_entire_buffer_binding()),
            ],
        );
        for (step, (pipeline, group, work)) in [
            (&pipelines.vertex, &vertex, count * 3),
            (&pipelines.geometry, &geometry, count),
        ]
        .into_iter()
        .enumerate()
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("geometry shader expansion"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            for (i, group) in groups.iter().take(3).enumerate() {
                if let Some(group) = group {
                    pass.set_bind_group(i as u32, group, &[]);
                }
            }
            pass.set_bind_group(3, group, &[]);
            pass.dispatch_workgroups(work.div_ceil(64), 1, 1);
            drop(pass);
            if step == 0 {
                between(encoder);
            }
        }
        group(
            &pipelines.raster_layout,
            &[(4, self.output.as_entire_buffer_binding())],
        )
    }
}

/// Binds the aligned source window while preserving the shader's original vertex and instance IDs.
fn storage_window(
    buffer: &wgpu::Buffer,
    range: std::ops::Range<u64>,
    alignment: u64,
) -> wgpu::BufferBinding<'_> {
    let offset = range.start / alignment * alignment;
    wgpu::BufferBinding {
        buffer,
        offset,
        size: std::num::NonZeroU64::new(range.end - offset),
    }
}

/// Expands a complete input draw in bounded batches and records each batch before reusing storage.
pub(crate) fn record(
    encoder: &mut wgpu::CommandEncoder,
    state: &mut ViewState,
    kind: DrawKind,
    write_alt: u32,
    depth: Option<(&wgpu::TextureView, [u32; 2])>,
    input: Input<'_>,
    groups: Option<&Groups>,
) {
    let pack = std::sync::Arc::clone(&state.pack);
    let Some(index) = pack.draws.iter().position(|draw| draw.kind == kind) else {
        return;
    };
    let Some(pipelines) = pack.draws[index].emulation.as_ref() else {
        return;
    };
    let Some(mut scratch) = state.geometry_shader.take() else {
        return;
    };
    let device = scratch.device.clone();
    let groups = groups
        .cloned()
        .unwrap_or_else(|| state.bindings.draws[index].groups[state.targets.parity].clone());
    for first in (0..input.count / 3).step_by(scratch.capacity as usize) {
        let count = scratch.capacity.min(input.count / 3 - first);
        let program = &pack.programs[pack.draws[index].program];
        let alt = super::targets::draw_reads(&pack, &pack.draws[index]).alt;
        state.targets.take_snapshots(encoder, program, alt);
        let output = scratch.expand(
            &device,
            encoder,
            pipelines,
            &groups,
            &input,
            first,
            count,
            &mut |encoder| state.targets.take_snapshots(encoder, program, alt),
        );
        if let Some((mut pass, _)) =
            super::frame_geometry::open_kind(encoder, state, kind, write_alt, depth)
        {
            for (i, group) in groups.iter().take(3).enumerate() {
                if let Some(group) = group {
                    pass.set_bind_group(i as u32, group, &[]);
                }
            }
            pass.set_bind_group(3, &output, &[]);
            pass.set_index_buffer(scratch.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed_indirect(&scratch.indirect, 0);
        }
    }
    state.geometry_shader = Some(scratch);
}
