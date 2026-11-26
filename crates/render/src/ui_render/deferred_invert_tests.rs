use super::*;
use render_model::{UI_BLEND_ALPHA, UI_BLEND_INVERT, UiScissor, UiTextureCatalog, UiTexturePage};
use std::sync::Arc;
use wgpu::util::DeviceExt;

fn batch(blend: u32, first_index: u32) -> (usize, UiRenderBatch, ()) {
    (
        first_index as usize / 6,
        UiRenderBatch::new(0, UiScissor::new(0, 0, 64, 64), first_index, 6, blend),
        (),
    )
}

/// One axis-aligned quad per `[x0, y0, x1, y1]`, six indices each.
fn input(quads: &[[f32; 4]]) -> UiRenderInput {
    let vertex = |position| render_model::UiRenderVertex {
        position,
        clip_z: 0.0,
        clip_w: 1.0,
        uv: [0.0; 2],
        color: [255; 4],
        style_flags: 0,
        alpha_cutoff: -1.0,
        model_light: 1.0,
        overlay_color: [0.0; 4],
    };
    let vertices: Vec<_> = quads
        .iter()
        .flat_map(|[x0, y0, x1, y1]| [[*x0, *y0], [*x1, *y0], [*x0, *y1], [*x1, *y1]].map(vertex))
        .collect();
    let indices: Vec<u32> = (0..quads.len() as u32)
        .flat_map(|quad| [0, 1, 2, 2, 1, 3].map(|corner| quad * 4 + corner))
        .collect();
    UiRenderInput {
        models: Default::default(),
        revision: 1,
        viewport_size: [64; 2],
        safe_area: [0; 4],
        vertices: vertices.into(),
        indices: indices.into(),
        batches: Arc::from([]),
        textures: Arc::new(
            UiTextureCatalog::new(
                vec![UiTexturePage::owned([1, 1], Arc::from([255; 4])).unwrap()],
                1,
            )
            .unwrap(),
        ),
    }
}

/// Only a leading invert followed by one deferred layer of plain 2D batches qualifies.
#[test]
fn only_a_leading_invert_before_a_plain_deferred_layer_is_postponed() {
    let hud = [
        batch(UI_BLEND_INVERT, 0),
        batch(UI_BLEND_ALPHA, 6),
        batch(UI_BLEND_ALPHA, 12),
    ];
    let plan = |batches: &[(usize, UiRenderBatch, ())], present| {
        segments::plan_ui_passes(batches, present)
    };
    assert_eq!(leading_invert(&plan(&hud, true), &hud), Some(0));
    // Without the output pass the layer composites itself; nothing follows to absorb it.
    assert_eq!(leading_invert(&plan(&hud, false), &hud), None);
    // A layer before the invert, a second invert, or no layer after it keeps the ordered passes.
    let layered_first = [
        batch(UI_BLEND_ALPHA, 0),
        batch(UI_BLEND_INVERT, 6),
        batch(UI_BLEND_ALPHA, 12),
    ];
    assert_eq!(
        leading_invert(&plan(&layered_first, true), &layered_first),
        None
    );
    let two = [
        batch(UI_BLEND_INVERT, 0),
        batch(UI_BLEND_INVERT, 6),
        batch(UI_BLEND_ALPHA, 12),
    ];
    assert_eq!(leading_invert(&plan(&two, true), &two), None);
    assert_eq!(leading_invert(&plan(&hud[..1], true), &hud[..1]), None);
    // Depth, projection, filters and model scopes are not plain 2D batches.
    for special in [
        hud[1].1.with_depth_test(true),
        hud[1].1.with_world_projection(true),
        hud[1].1.with_filter_group(Some(1)),
    ] {
        let mixed = [hud[0], (1, special, ()), hud[2]];
        assert_eq!(leading_invert(&plan(&mixed, true), &mixed), None);
    }
}

/// Bounds come from the publication's vertices and scissor; overlap within a pixel defers nothing.
#[test]
fn inverts_defer_only_when_their_pixels_miss_every_layered_batch() {
    let input = input(&[
        [30.0, 30.0, 34.0, 34.0],
        [0.0, 56.0, 64.0, 64.0],
        [34.5, 30.0, 40.0, 34.0],
    ]);
    let crosshair = batch(UI_BLEND_INVERT, 0).1;
    let hotbar = batch(UI_BLEND_ALPHA, 6).1;
    let touching = batch(UI_BLEND_ALPHA, 12).1;
    assert_eq!(
        batch_bounds(&input, &crosshair),
        Some([30.0, 30.0, 34.0, 34.0])
    );
    assert!(disjoint(&input, &crosshair, [&hotbar]));
    assert!(!disjoint(&input, &crosshair, [&hotbar, &touching]));
    // A scissor that removes the touching quad's pixels makes it disjoint again.
    let clipped = UiRenderBatch {
        scissor: UiScissor::new(41, 0, 23, 64),
        ..touching
    };
    assert_eq!(batch_bounds(&input, &clipped), None);
    assert!(disjoint(&input, &crosshair, [&hotbar, &clipped]));
}

#[test]
fn only_matching_or_eight_bit_srgb_targets_blend_alike() {
    use TextureFormat::*;
    assert!(blends_alike(Rgba8UnormSrgb, Bgra8UnormSrgb));
    assert!(blends_alike(Rgba16Float, Rgba16Float));
    assert!(!blends_alike(Rgba8UnormSrgb, Bgra8Unorm));
    assert!(!blends_alike(Rgba16Float, Rgba8UnormSrgb));
}

/// Draws the production composite of a `ui` layer over an sRGB `scene` into `output`, with the
/// production invert blended over the scene before or over the output after it.
fn composite_and_invert(
    gpu: &crate::gpu_snapshot::Gpu,
    ui: &[[u8; 4]],
    scene: &[[u8; 4]],
    inverted: std::ops::Range<u32>,
    invert_first: bool,
    output: wgpu::TextureFormat,
) -> Vec<u8> {
    let width = ui.len() as u32;
    let size = wgpu::Extent3d {
        width,
        height: 1,
        depth_or_array_layers: 1,
    };
    let texture = |format, usage, bytes: &[u8]| {
        gpu.device.create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytes,
        )
    };
    let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_SRC;
    let layer = texture(
        super::composite::UI_LAYER_FORMAT,
        attachment,
        bytemuck::cast_slice(ui),
    );
    let scene = texture(
        wgpu::TextureFormat::Rgba8UnormSrgb,
        attachment,
        bytemuck::cast_slice(scene),
    );
    let out = texture(output, attachment, &vec![0; ui.len() * 4]);
    let composite_module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("production UI composite"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../ui_composite.wgsl").into()),
        });
    let invert_module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("crosshair colour"),
        source: wgpu::ShaderSource::Wgsl(
            "@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
                return vec4(vec2(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0, 0.0, 1.0);
            }
            @fragment fn fragment() -> @location(0) vec4<f32> { return vec4(0.9, 0.8, 0.7, 0.85); }"
                .into(),
        ),
    });
    let pipeline = |module: &wgpu::ShaderModule, vertex, fragment, format, blend| {
        gpu.device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: None,
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
    };
    let invert_blend = Some(super::pipeline::ui_invert_blend_state());
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let draw_invert = |encoder: &mut wgpu::CommandEncoder, target: &wgpu::Texture| {
        let view = target.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline(
            &invert_module,
            "vertex",
            "fragment",
            target.format(),
            invert_blend,
        ));
        pass.set_scissor_rect(inverted.start, 0, inverted.end - inverted.start, 1);
        pass.draw(0..3, 0..1);
    };
    if invert_first {
        draw_invert(&mut encoder, &scene);
    }
    {
        let composite = pipeline(
            &composite_module,
            "composite_vertex",
            "composite_fragment",
            output,
            None,
        );
        let views = [&layer, &scene].map(|texture| texture.create_view(&Default::default()));
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &composite.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[1]),
                },
            ],
        });
        let view = out.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations::default(),
            })],
            ..Default::default()
        });
        pass.set_pipeline(&composite);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
    if !invert_first {
        draw_invert(&mut encoder, &out);
    }
    gpu.queue.submit([encoder.finish()]);
    let device = bevy::render::renderer::RenderDevice::from(gpu.device.clone());
    let queue = bevy::render::renderer::RenderQueue::new(gpu.queue.clone());
    let mut pixels = crate::render_test_support::read_rgba8_layer(&device, &queue, &out, 0);
    if output == wgpu::TextureFormat::Bgra8UnormSrgb {
        pixels.chunks_mut(4).for_each(|pixel| pixel.swap(0, 2));
    }
    pixels
}

/// Over pixels the layer leaves empty, inverting the scene and then compositing equals
/// compositing and then inverting the output, byte for byte, for either 8-bit sRGB output.
#[test]
fn inverting_after_the_composite_matches_inverting_the_scene_where_the_layer_is_empty() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("deferred HUD invert") else {
        return;
    };
    let scene: Vec<[u8; 4]> = (0..=255u32)
        .map(|value| {
            [
                value as u8,
                (255 - value) as u8,
                (value * 37 % 256) as u8,
                (value * 11 % 256) as u8,
            ]
        })
        .collect();
    // The layer is empty over the inverted first half and translucent or opaque elsewhere.
    let ui: Vec<[u8; 4]> = (0..256u32)
        .map(|x| {
            if x < 128 {
                [0; 4]
            } else if x % 2 == 0 {
                [40, 60, 80, 128]
            } else {
                [200, 10, 90, 255]
            }
        })
        .collect();
    let ordered = composite_and_invert(
        &gpu,
        &ui,
        &scene,
        0..128,
        true,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    for output in [
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    ] {
        let deferred = composite_and_invert(&gpu, &ui, &scene, 0..128, false, output);
        assert_eq!(ordered, deferred, "{output:?}");
    }
    // Over a non-empty layer the order matters, which is why deferral requires disjoint bounds.
    let over_layer = |invert_first| {
        composite_and_invert(
            &gpu,
            &ui,
            &scene,
            128..256,
            invert_first,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        )
    };
    assert_ne!(over_layer(true), over_layer(false));
    // The invert changes those pixels, so the equality is not vacuous.
    let plain = composite_and_invert(
        &gpu,
        &ui,
        &scene,
        0..0,
        true,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    assert_ne!(ordered[..128 * 4], plain[..128 * 4]);
}
