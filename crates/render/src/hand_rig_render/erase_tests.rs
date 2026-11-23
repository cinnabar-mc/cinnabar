use super::*;
use bevy::render::render_resource::{LoadOp, StoreOp};

#[test]
fn erase_keeps_the_draws_shaders_and_rasterization_but_only_writes_depth() {
    for samples in [1, 2, 4, 8] {
        for (hdr, encoded) in [(false, false), (false, true), (true, false)] {
            let draw = specialized_pipeline(hand_rig_layout(), samples, hdr, encoded);
            let erase = erase_pipeline(draw.clone());
            assert_eq!(erase.vertex.shader, draw.vertex.shader);
            assert_eq!(erase.vertex.entry_point, draw.vertex.entry_point);
            assert_eq!(erase.vertex.shader_defs, draw.vertex.shader_defs);
            let (erase_fragment, draw_fragment) = (erase.fragment.unwrap(), draw.fragment.unwrap());
            assert_eq!(erase_fragment.shader, draw_fragment.shader);
            assert_eq!(erase_fragment.entry_point, draw_fragment.entry_point);
            assert_eq!(erase_fragment.shader_defs, draw_fragment.shader_defs);
            assert_eq!(erase.primitive, draw.primitive);
            assert_eq!(erase.multisample, draw.multisample);
            let target = erase_fragment.targets[0].as_ref().unwrap();
            assert_eq!(
                target.format,
                draw_fragment.targets[0].as_ref().unwrap().format
            );
            assert_eq!(target.write_mask, ColorWrites::empty());
            let depth = erase.depth_stencil.unwrap();
            assert_eq!(depth.format, draw.depth_stencil.unwrap().format);
            assert_eq!(depth.depth_compare, Some(CompareFunction::Always));
            assert_eq!(depth.depth_write_enabled, Some(true));
        }
    }
}

#[test]
fn only_immediate_mode_desktop_backends_erase() {
    for backend in [wgpu::Backend::Vulkan, wgpu::Backend::Dx12] {
        assert_eq!(admitted(backend), !cfg!(target_os = "android"));
    }
    for backend in [
        wgpu::Backend::Metal,
        wgpu::Backend::Gl,
        wgpu::Backend::BrowserWebGpu,
        wgpu::Backend::Noop,
    ] {
        assert!(!admitted(backend));
    }
}

#[test]
fn hand_depth_is_loaded_only_after_an_erased_pass() {
    let cleared = DepthCleared::default();
    let first = cleared.operations(true);
    assert_eq!(first.load, LoadOp::Clear(CLEARED_DEPTH));
    assert_eq!(first.store, StoreOp::Store);
    cleared.record(true);
    let erased = cleared.operations(true);
    assert_eq!(erased.load, LoadOp::Load);
    assert_eq!(erased.store, StoreOp::Store);
    // A pass without the erase pipeline discards its depth, so the next one clears again.
    let unerased = cleared.operations(false);
    assert_eq!(unerased.load, LoadOp::Load);
    assert_eq!(unerased.store, StoreOp::Discard);
    cleared.record(false);
    assert_eq!(cleared.operations(true).load, LoadOp::Clear(CLEARED_DEPTH));
}

/// Instance 0 is a near left half that discards every other pixel, instance 1 a far full screen.
const ERASE_FIXTURE: &str = "
struct Out { @builtin(position) position: vec4<f32>, @location(0) @interpolate(flat) instance: u32 }
@vertex fn vertex(@builtin(vertex_index) v: u32, @builtin(instance_index) i: u32) -> Out {
    let corner = vec2(f32(v & 1u), f32((v >> 1u) & 1u)) * 4.0 - 1.0;
    let x = select(corner.x, min(corner.x, 0.0), i == 0u);
    return Out(vec4(x, corner.y, select(0.2, 0.9, i == 0u), 1.0), i);
}
@fragment fn fragment(in: Out) -> @location(0) vec4<f32> {
    let pixel = vec2<u32>(in.position.xy);
    if (in.instance == 0u && ((pixel.x + pixel.y) & 1u) == 1u) { discard; }
    return select(vec4(0.0, 1.0, 0.0, 1.0), vec4(1.0, 0.0, 0.0, 1.0), in.instance == 0u);
}";

/// Requests a headless adapter for real depth testing across passes.
fn erase_device() -> Option<(RenderDevice, bevy::render::renderer::RenderQueue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("missing fixture: headless GPU adapter for the hand depth erase");
        return None;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&Default::default()))
        .expect("hand depth erase device");
    Some((
        RenderDevice::from(device),
        bevy::render::renderer::RenderQueue::new(queue),
    ))
}

/// A depth erase leaves the attachment as a clear would, so a later, farther draw that loads it
/// is not occluded by the earlier one, including around fragments the earlier draw discarded.
#[test]
fn erased_hand_depth_loads_like_a_cleared_attachment() {
    let Some((device, queue)) = erase_device() else {
        return;
    };
    const SIDE: u32 = 16;
    // Every device supports four samples of these formats.
    const SAMPLES: u32 = 4;
    let gpu = device.wgpu_device();
    let module = gpu.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("hand depth erase fixture"),
        source: wgpu::ShaderSource::Wgsl(ERASE_FIXTURE.into()),
    });
    let pipeline = |compare, write_mask| {
        RenderPipeline::from(gpu.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hand depth erase fixture"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(compare),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: SAMPLES,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask,
                })],
            }),
            multiview_mask: None,
            cache: None,
        }))
    };
    let draw = pipeline(CompareFunction::GreaterEqual, ColorWrites::ALL);
    let erase = pipeline(CompareFunction::Always, ColorWrites::empty());
    let extent = Extent3d {
        width: SIDE,
        height: SIDE,
        depth_or_array_layers: 1,
    };
    let texture = |format, samples, usage| {
        gpu.create_texture(&wgpu::TextureDescriptor {
            label: Some("hand depth erase fixture"),
            size: extent,
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
    let samples =
        texture(wgpu::TextureFormat::Rgba8Unorm, SAMPLES, attachment).create_view(&default());
    let resolved = texture(
        wgpu::TextureFormat::Rgba8Unorm,
        1,
        attachment | wgpu::TextureUsages::COPY_SRC,
    );
    let resolved_view = resolved.create_view(&default());
    // Encodes one hand-style pass: `instance` draws, then optionally erases its own depth.
    let frame = |depth: &wgpu::TextureView, operations, instance: u32, erases: bool| {
        let mut encoder = device.create_command_encoder(&default());
        let colors = [Some(wgpu::RenderPassColorAttachment {
            view: &samples,
            depth_slice: None,
            resolve_target: Some(&resolved_view),
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Discard,
            },
        })];
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &colors,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(operations),
                stencil_ops: None,
            }),
            ..default()
        });
        let mut pass = bevy::render::render_phase::TrackedRenderPass::new(&device, pass);
        pass.set_render_pipeline(&draw);
        pass.draw(0..4, instance..instance + 1);
        if erases {
            erase_draw(&mut pass, &erase, extent, 0..4, instance..instance + 1);
        }
        drop(pass);
        queue.submit([encoder.finish()]);
        crate::render_test_support::read_rgba8_layer(&device, &queue, &resolved, 0)
    };
    let new_depth = || texture(CORE_3D_DEPTH_FORMAT, SAMPLES, attachment).create_view(&default());

    let reference = frame(
        &new_depth(),
        DepthCleared::default().operations(false),
        1,
        false,
    );
    assert!(reference.chunks(4).all(|pixel| pixel == [0, 255, 0, 255]));

    let depth = new_depth();
    let cleared = DepthCleared::default();
    let near = frame(&depth, cleared.operations(true), 0, true);
    assert_eq!(
        &near[..4],
        &[255, 0, 0, 255],
        "the near draw covers the left half"
    );
    cleared.record(true);
    let operations = cleared.operations(true);
    assert_eq!(operations.load, LoadOp::Load);
    assert_eq!(frame(&depth, operations, 1, true), reference);
}
