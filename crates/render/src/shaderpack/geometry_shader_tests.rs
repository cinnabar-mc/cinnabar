//! Headless rendering of the original triangle-expansion fixture.

#![allow(
    clippy::print_stderr,
    reason = "names missing GPU fixtures when tests skip"
)]

use super::draws::{DrawKind, GeometryDraw};
use super::geometry::TerrainLayout;
use super::geometry_shader::*;
use std::borrow::Cow;
use std::sync::mpsc;
use wgpu::util::DeviceExt;

/// Small binding limit used to exercise windows into source buffers larger than one binding.
const STORAGE_LIMIT: u64 = 4096;

#[test]
fn emitted_triangles_render_separately_with_their_varyings() {
    for (packed, cpu_indices) in [(false, false), (true, false), (true, true)] {
        render_emission(packed, cpu_indices);
    }
}

#[test]
fn failed_geometry_storage_allocation_rejects_the_view_recoverably() {
    let Some(gpu) = super::tests::gpu() else {
        eprintln!("missing fixture: GPU adapter for geometry shader expansion");
        return;
    };
    let Some(pack) = load_fixture(&gpu) else {
        return;
    };
    let failed = std::rc::Rc::new(std::cell::Cell::new(false));
    let signal = std::rc::Rc::clone(&failed);
    let _failure = crate::gpu_allocation::inject(move |label, _| {
        let fail = label == SCRATCH_ALLOCATION;
        signal.set(signal.get() || fail);
        fail
    });
    let result = super::passes::ViewState::new(&gpu.device, pack, [32, 32]);
    assert!(failed.get(), "the expansion allocation was attempted");
    assert!(matches!(result, Err(error) if error.contains("GPU memory")));
}

/// Loads the complete fixture through the client planner, or names a missing device capability.
fn load_fixture(
    gpu: &super::pipelines::GpuContext,
) -> Option<std::sync::Arc<super::loader::PackGpu>> {
    if let Err(reason) = super::kernels::validate_device(&gpu.device.limits()) {
        eprintln!("missing fixture: GPU adapter supporting complete shader-pack kernels: {reason}");
        return None;
    }
    let source = fixture();
    let loaded = super::loader::build(
        &source,
        "geometry fixture",
        super::plugin::JAVA_DIMENSIONS[0],
        &Default::default(),
        None,
        gpu,
        &Default::default(),
        &Default::default(),
    )
    .expect("the original geometry pack loads without dropping its shadow program");
    assert!(loaded.warnings.is_empty(), "{}", loaded.warnings);
    assert!(
        loaded
            .draws
            .iter()
            .any(|draw| { draw.kind == DrawKind::ShadowSolid && draw.emulation.is_some() })
    );
    Some(std::sync::Arc::new(loaded))
}

/// Opens the original fixture used by both the whole-pack loader and isolated expansion tests.
fn fixture() -> shaderpack::ShaderPack {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../shaderpack/tests/fixtures/geometry");
    shaderpack::ShaderPack::open(&path).expect("original geometry fixture")
}

/// Checks both actor vertices and indexed packed terrain through the actual compute pipelines.
fn render_emission(packed: bool, cpu_indices: bool) {
    let Some(gpu) = super::tests::gpu() else {
        eprintln!("missing fixture: GPU adapter for geometry shader expansion");
        return;
    };
    let mut limits = gpu.adapter.limits();
    limits.max_storage_buffer_binding_size = STORAGE_LIMIT;
    let (device, queue) =
        bevy::tasks::block_on(gpu.adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("geometry fixture bounded bindings"),
            required_features: gpu.device.features(),
            required_limits: limits,
            ..Default::default()
        }))
        .expect("small storage binding device");
    let gpu = super::pipelines::GpuContext {
        device,
        queue,
        adapter: gpu.adapter,
    };
    let source = fixture();
    let translated = source.translate(&Default::default());
    let program = translated
        .programs
        .iter()
        .find(|p| p.program.name == "shadow")
        .unwrap();
    let reflection = program.reflection.as_ref().unwrap_or_else(|| {
        panic!(
            "geometry fixture failed: {:?}",
            program.all_diagnostics().collect::<Vec<_>>()
        )
    });
    let stage = |kind| {
        program
            .stages
            .iter()
            .find(|s| s.stage == kind)
            .unwrap()
            .result
            .as_ref()
            .unwrap()
    };
    let (vertex, geometry, fragment) = (
        stage(shaderpack::StageKind::Vertex),
        stage(shaderpack::StageKind::Geometry),
        stage(shaderpack::StageKind::Fragment),
    );
    let layouts = [None, None, None, None];
    let terrain = if packed {
        TerrainLayout::compact(&reflection.vertex_inputs)
    } else {
        TerrainLayout::WIDE
    };
    let kind = if packed {
        DrawKind::ShadowSolid
    } else {
        DrawKind::Entities
    };
    let pipelines = pipelines(
        &gpu.device,
        kind,
        terrain,
        &layouts,
        reflection,
        vertex,
        geometry,
        program.geometry.as_ref().unwrap(),
    )
    .unwrap();
    let fragment = super::pipelines::pack_shader_module(
        &gpu.device,
        wgpu::ShaderModuleDescriptor {
            label: Some("original geometry fixture fragment"),
            source: wgpu::ShaderSource::Naga(Cow::Owned(fragment.module.clone())),
        },
    );
    let targets = [Some(wgpu::ColorTargetState {
        format: wgpu::TextureFormat::Rgba8Unorm,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let raster = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("geometry fixture raster"),
            layout: Some(&pipelines.raster_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &pipelines.raster_vertex,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fragment,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        });
    let draws = [GeometryDraw {
        kind,
        program: 0,
        pipeline: raster.clone(),
        emulation: Some(pipelines),
        outputs: vec![Some(0)],
        layout: None,
        alpha_test: None,
    }];
    let mut scratch = Scratch::new(&gpu.device, &draws).unwrap();
    let origin = super::geometry::draw_origin([0; 3], [0; 3]);
    let positions = [[-1.0_f32, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]];
    let mut words = if packed {
        let stride = (terrain.stride() / 4) as usize;
        let position_word = ((terrain.expand_words()[0] >> 16) & 0xff) as usize;
        let color_word = terrain.word(super::geometry::COLOR).unwrap() as usize;
        let mut words = vec![0u32; stride * 5];
        for (index, position) in positions.iter().enumerate() {
            let values = std::array::from_fn::<_, 3, _>(|axis| {
                ((position[axis] - origin[axis]) / origin[3]).round() as u32
            });
            let base = (index + 2) * stride;
            words[base + position_word] = values[0] | values[1] << 16;
            words[base + position_word + 1] = values[2];
            words[base + color_word] = u32::MAX;
        }
        words
    } else {
        positions
            .into_iter()
            .flat_map(|position| {
                let mut vertex =
                    super::geometry::host_vertex(position, [1.0; 4], [0.0, 0.0, 1.0]).to_vec();
                vertex.resize(super::actors::layout::ACTOR_VERTEX_WORDS as usize, 0);
                vertex
            })
            .collect::<Vec<_>>()
    };
    if !packed || cpu_indices {
        words.resize((STORAGE_LIMIT / 4 * 2) as usize, 0);
    }
    let vertices = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("original geometry fixture vertices"),
            contents: bytemuck::cast_slice(&words),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let first_index = (STORAGE_LIMIT / 4 + 2) as usize;
    let mut element_values = vec![0u32; first_index];
    element_values.extend([0, 1, 2]);
    let elements = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("indexed geometry fixture"),
            contents: bytemuck::cast_slice(&element_values),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let instance = if packed { 17 } else { 0 };
    let mut origins = vec![0.0_f32; (STORAGE_LIMIT / 4 * 2) as usize];
    let origin_start = instance as usize * 4;
    origins[origin_start..origin_start + 4].copy_from_slice(&origin);
    let origins = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("geometry fixture draw origin"),
            contents: bytemuck::cast_slice(&origins),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let output = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("geometry fixture frame"),
        size: wgpu::Extent3d {
            width: 32,
            height: 32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("geometry fixture readback"),
        size: 256 * 32,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let group = scratch.expand(
        &gpu.device,
        &mut encoder,
        draws[0].emulation.as_ref().unwrap(),
        &[None, None, None, None],
        &Input {
            vertices: &vertices,
            indices: packed.then_some(&elements),
            index_values: cpu_indices.then_some(element_values.as_slice()),
            origins: packed.then_some(&origins),
            first: if packed { first_index as u32 } else { 0 },
            count: 3,
            base_vertex: if packed { 2 } else { 0 },
            instance,
        },
        0,
        1,
        &mut |_| {},
    );
    let view = output.create_view(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("geometry fixture capture"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&raster);
        pass.set_bind_group(3, &group, &[]);
        pass.set_index_buffer(scratch.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed_indirect(&scratch.indirect, 0);
    }
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(32),
            },
        },
        output.size(),
    );
    scratch.finish();
    gpu.queue.submit([encoder.finish()]);
    let (send, receive) = mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap();
        });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    receive.recv().unwrap().unwrap();
    let bytes = readback.slice(..).get_mapped_range().unwrap();
    let pixel = |x: usize| &bytes[16 * 256 + x * 4..16 * 256 + x * 4 + 4];
    let blue = (255.0 / (1.0 + instance as f32)).round() as u8;
    assert_eq!(pixel(8), [255, 0, blue, 255]);
    assert_eq!(pixel(24), [0, 255, blue, 255]);
    assert_eq!(
        pixel(16),
        [0, 0, 0, 255],
        "EndPrimitive must not connect the strips"
    );
}
