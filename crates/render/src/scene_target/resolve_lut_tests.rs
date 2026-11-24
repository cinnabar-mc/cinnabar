use super::*;
use wgpu::util::DeviceExt;

const SIDE: u32 = 64;

/// Per-sample codes from a hash of pixel, sample and channel, covering every code pair over many
/// pixels, written through the UNORM view exactly as stored bytes.
const SCENE: &str = "
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    return vec4(vec2(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0, 0.0, 1.0);
}
fn hash(value: u32) -> u32 {
    var x = value * 747796405u + 2891336453u;
    x = ((x >> ((x >> 28u) + 4u)) ^ x) * 277803737u;
    return (x >> 22u) ^ x;
}
@fragment fn samples(@builtin(position) position: vec4<f32>, @builtin(sample_index) sample: u32)
    -> @location(0) vec4<f32> {
    let pixel = vec2<u32>(position.xy);
    let seed = (pixel.y * 64u + pixel.x) * 2u + sample;
    // Many pixels hold equal samples, as covered interiors do.
    let uniform_pixel = (hash(pixel.y * 64u + pixel.x) & 3u) == 0u;
    let key = select(seed, (pixel.y * 64u + pixel.x) * 2u, uniform_pixel);
    return vec4<f32>(vec4<u32>(hash(key) & 255u, hash(key + 9u) & 255u, hash(key + 17u) & 255u, hash(key + 31u) & 255u)) / 255.0;
}";

/// Two-sample scene contents and the device's render-pass resolve of them.
struct Scene {
    samples: wgpu::Texture,
    resolved: wgpu::Texture,
}

fn scene(gpu: &wgpu::Device, queue: &wgpu::Queue) -> Scene {
    let size = wgpu::Extent3d {
        width: SIDE,
        height: SIDE,
        depth_or_array_layers: 1,
    };
    let samples = gpu.create_texture(&wgpu::TextureDescriptor {
        label: Some("random two-sample scene"),
        size,
        mip_level_count: 1,
        sample_count: 2,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
    });
    let resolved = gpu.create_texture(&wgpu::TextureDescriptor {
        label: Some("resolved scene"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let module = gpu.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(SCENE.into()),
    });
    let pipeline = gpu.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: 2,
            ..Default::default()
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("samples"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let encoded = samples.create_view(&Default::default());
    let linear = samples.create_view(&wgpu::TextureViewDescriptor {
        format: Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        ..Default::default()
    });
    let resolved_view = resolved.create_view(&Default::default());
    let mut encoder = gpu.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &encoded,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations::default(),
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.draw(0..3, 0..1);
    }
    // The scene's end-of-frame resolve, as `SceneTarget::finish` encodes it.
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &linear,
            depth_slice: None,
            resolve_target: Some(&resolved_view),
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    queue.submit([encoder.finish()]);
    Scene { samples, resolved }
}

/// Runs a production composite entry over `ui` into an sRGB target, binding `scene` as either
/// the resolved texture or the samples with the table.
fn composite(
    gpu: &wgpu::Device,
    queue: &wgpu::Queue,
    entry: &str,
    ui: &wgpu::TextureView,
    scene: &[(u32, &wgpu::TextureView)],
) -> wgpu::Texture {
    let module = gpu.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production UI composite"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../ui_composite.wgsl").into()),
    });
    let pipeline = gpu.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("composite_vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let mut entries = vec![wgpu::BindGroupEntry {
        binding: 0,
        resource: wgpu::BindingResource::TextureView(ui),
    }];
    entries.extend(scene.iter().map(|(binding, view)| wgpu::BindGroupEntry {
        binding: *binding,
        resource: wgpu::BindingResource::TextureView(view),
    }));
    let group = gpu.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &entries,
    });
    let output = gpu.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = output.create_view(&Default::default());
    let mut encoder = gpu.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations::default(),
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
    queue.submit([encoder.finish()]);
    output
}

/// Compositing over the scene's samples through the device's tabulated resolve writes the same
/// bytes as resolving the scene with a render pass and compositing over the resolved texture,
/// under empty, translucent and opaque UI alike.
#[test]
fn fused_composite_matches_resolving_then_compositing() {
    let Some(fixture) = crate::gpu_snapshot::Gpu::for_fixture_with(
        "tabulated two-sample resolve",
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
    ) else {
        return;
    };
    let (gpu, queue) = (&fixture.device, &fixture.queue);
    if !gpu
        .features()
        .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
    {
        eprintln!("missing fixture: two-sample render targets for the resolve table");
        return;
    }
    let table = table(gpu, queue);
    let scene = scene(gpu, queue);
    let ui_pixels: Vec<u8> = (0..SIDE * SIDE)
        .flat_map(|pixel| match pixel % 3 {
            0 => [0, 0, 0, 0],
            1 => [30, 70, 110, 140],
            _ => [210, 20, 90, 255],
        })
        .collect();
    let ui = gpu
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: SIDE,
                    height: SIDE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: crate::ui_render::composite::UI_LAYER_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &ui_pixels,
        )
        .create_view(&Default::default());
    let resolved = scene.resolved.create_view(&Default::default());
    let samples = scene.samples.create_view(&Default::default());
    let ordered = composite(gpu, queue, "composite_fragment", &ui, &[(1, &resolved)]);
    let fused = composite(
        gpu,
        queue,
        "fused_composite_fragment",
        &ui,
        &[(3, &samples), (4, &table)],
    );
    let device = bevy::render::renderer::RenderDevice::from(gpu.clone());
    let queue = bevy::render::renderer::RenderQueue::new(queue.clone());
    let read = |texture: &wgpu::Texture| {
        crate::render_test_support::read_rgba8_layer(&device, &queue, texture, 0)
    };
    let (ordered, fused) = (read(&ordered), read(&fused));
    let differing = ordered
        .chunks(4)
        .zip(fused.chunks(4))
        .filter(|(ordered, fused)| ordered != fused)
        .count();
    assert_eq!(differing, 0, "of {} pixels", SIDE * SIDE);
    // The scene differs between pixels, so equal outputs are not a constant.
    assert!(
        ordered
            .chunks(4)
            .collect::<std::collections::HashSet<_>>()
            .len()
            > 1000
    );
}
