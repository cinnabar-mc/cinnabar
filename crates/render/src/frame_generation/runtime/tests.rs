use super::*;
use crate::frame_generation::optical_flow::tests::{device, pattern, read};

#[test]
fn foreground_composition_shader_is_valid() {
    let source = format!(
        "{}\n{}",
        include_str!("../../ui_composite.wgsl"),
        include_str!("composition.wgsl")
    );
    let module = naga::front::wgsl::parse_str(&source).expect("composition shader");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("valid foreground composition");
}

/// Allocates a complete synthetic image with upload, render and readback usages.
fn texture(
    device: &wgpu::Device,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("frame generation composition fixture"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | if samples == 1 && !format.is_depth_stencil_format() {
                wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST
            } else {
                wgpu::TextureUsages::empty()
            },
        view_formats: &[],
    })
}

/// Uploads original encoded fixture pixels without altering their colour space.
fn upload(queue: &wgpu::Queue, image: &wgpu::Texture, pixels: &[u8]) {
    queue.write_texture(
        image.as_image_copy(),
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(image.width() * 4),
            rows_per_image: None,
        },
        image.size(),
    );
}

/// Uses the production fullscreen vertex and an explicitly selected fixture or UI fragment.
fn pipeline(
    device: &wgpu::Device,
    source: &str,
    entry: &str,
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("composition fixture"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(entry),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("composite_vertex"),
            buffers: &[],
            compilation_options: default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(entry),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: default(),
        }),
        primitive: default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: samples,
            ..default()
        },
        multiview_mask: None,
        cache: None,
    })
}

/// Draws stationary labels and a hand, including their independent MSAA coverage.
fn foreground(device: &wgpu::Device, queue: &wgpu::Queue, ready: &gpu::Ready) {
    let source = format!("{}\nconst WIDTH: f32 = {}.0;\n
        @fragment fn labels(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {{
            return select(vec4(0.0), vec4(1.0,0.0,0.0,1.0), p.x >= WIDTH * 0.25 && p.x < WIDTH * 0.75);
        }}
        @fragment fn hand(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {{
            return select(vec4(0.0), vec4(0.0,1.0,0.0,1.0), p.x >= WIDTH * 0.5);
        }}", include_str!("../../ui_composite.wgsl"), ready.key.size.render[0]);
    let mut encoder = device.create_command_encoder(&default());
    for (layer, entry) in [(&ready.labels, "labels"), (&ready.hand, "hand")] {
        let pipeline = pipeline(
            device,
            &source,
            entry,
            layer._texture.format(),
            ready.key.samples,
        );
        let mut attachment = layer.attachment();
        attachment.ops.load = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        attachment.resolve_target = layer.resolved.as_ref().map(|(_, view)| view);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(attachment)],
            ..default()
        });
        pass.set_pipeline(&pipeline);
        pass.draw(0..3, 0..1);
    }
    queue.submit([encoder.finish()]);
}

/// Composes current foreground followed by the same native HUD shader used for actual output.
fn composed(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    ready: &gpu::Ready,
    source: &wgpu::TextureView,
    hud: &wgpu::TextureView,
) -> Vec<u8> {
    let size = ready.key.size.display;
    let scene = texture(device, size, render_scene::SCENE_COLOR_FORMAT, 1);
    let scene_view = scene.create_view(&default());
    let output = texture(device, size, render_scene::SCENE_COLOR_FORMAT, 1);
    let output_view = output.create_view(&default());
    let mut encoder = device.create_command_encoder(&default());
    ready.compose(device, &mut encoder, source, &scene_view);
    let ui = pipeline(
        device,
        include_str!("../../ui_composite.wgsl"),
        "composite_fragment",
        render_scene::SCENE_COLOR_FORMAT,
        1,
    );
    let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &ui.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(hud),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&scene_view),
            },
        ],
    });
    gpu::encode(&mut encoder, &ui, &binding, &output_view);
    queue.submit([encoder.finish()]);
    read(device, queue, &output, 4)
}

/// Saves optional original fixture captures outside the source checkout for visual review.
fn capture(name: &str, size: [u32; 2], pixels: &[u8]) {
    let Some(directory) = std::env::var_os("CINNABAR_FRAME_GENERATION_CAPTURE_DIR") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    image::save_buffer(
        directory.join(format!("{name}.png")),
        pixels,
        size[0],
        size[1],
        image::ColorType::Rgba8,
    )
    .unwrap();
}

#[test]
fn real_and_generated_worlds_keep_native_ui_and_current_foreground_in_order() {
    let Some((device, queue)) = device() else {
        return;
    };
    let display = [256, 64];
    let hud = texture(&device, display, wgpu::TextureFormat::Rgba8Unorm, 1);
    let mut pixels = vec![0; (display[0] * display[1] * 4) as usize];
    for y in 0..display[1] {
        for x in 0..display[0] {
            let pixel = &mut pixels[((y * display[0] + x) * 4) as usize..][..4];
            pixel.copy_from_slice(if x == display[0] / 2 {
                &[255, 255, 255, 255]
            } else if x < display[0] / 8 {
                &[0, 0, 255, 255]
            } else if y < 4 {
                &[128, 128, 0, 128]
            } else {
                &[0; 4]
            });
        }
    }
    upload(&queue, &hud, &pixels);
    for render in [
        display,
        display.map(|v| v / 2),
        display.map(|v| (v / 10).max(1)),
    ] {
        for samples in [1, 4] {
            let depth = texture(&device, render, wgpu::TextureFormat::Depth32Float, samples);
            let source =
                bevy::render::render_resource::TextureView::from(depth.create_view(&default()));
            let key = gpu::Key {
                device: device.clone(),
                size: FrameGenerationSize { display, render },
                samples,
                output: render_scene::SCENE_COLOR_FORMAT,
                depth: source.id(),
            };
            let ready = gpu::Ready::new(RenderDevice::from(device.clone()), key, source)
                .expect("prepared composition fixture");
            foreground(&device, &queue, &ready);
            let mut encoder = device.create_command_encoder(&default());
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &ready.depth.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..default()
            });
            queue.submit([encoder.finish()]);
            let current = pattern(display, 100, [8, 0]);
            let real = texture(&device, display, render_scene::SCENE_COLOR_FORMAT, 1);
            upload(&queue, &real, &current);
            let generated = {
                let mut temporal = ready.temporal.lock().unwrap();
                let frame = FrameGenerationInput {
                    color: &ready.world,
                    depth: &ready.depth._texture,
                    previous_clip_from_clip: Mat4::IDENTITY,
                    jitter: [0.0; 2],
                    near: 0.05,
                    far: f32::INFINITY,
                    fov_y: 1.2,
                    reset: false,
                };
                upload(&queue, &ready.world, &pattern(display, 100, [0, 0]));
                assert!(
                    temporal
                        .generator
                        .generate(&device, &queue, &frame)
                        .unwrap()
                        .history_reset
                );
                upload(&queue, &ready.world, &current);
                let image = temporal
                    .generator
                    .generate(&device, &queue, &frame)
                    .unwrap();
                assert!(!image.history_reset);
                image.image.clone()
            };
            let generated_native = texture(&device, display, render_scene::SCENE_COLOR_FORMAT, 1);
            let mut encoder = device.create_command_encoder(&default());
            ready.blit(
                &device,
                &mut encoder,
                &ready.copy_generated,
                &generated,
                &generated_native.create_view(&default()),
            );
            queue.submit([encoder.finish()]);
            let hud_view = hud.create_view(&default());
            let real_pixels = composed(
                &device,
                &queue,
                &ready,
                &real.create_view(&default()),
                &hud_view,
            );
            let generated_pixels = composed(
                &device,
                &queue,
                &ready,
                &generated_native.create_view(&default()),
                &hud_view,
            );
            for output in [&real_pixels, &generated_pixels] {
                let pixel = |x: usize, y: usize| &output[(y * display[0] as usize + x) * 4..][..4];
                assert_eq!(pixel(10, 32), [0, 0, 255, 255], "opaque native HUD");
                assert_eq!(
                    pixel(100, 32),
                    [255, 0, 0, 255],
                    "unwarped labels over world"
                );
                assert_eq!(pixel(180, 32), [0, 255, 0, 255], "current hand over labels");
                assert_eq!(
                    pixel(128, 32),
                    [255, 255, 255, 255],
                    "one-native-pixel HUD over hand"
                );
                let translucent = pixel(180, 1);
                assert!(
                    (i16::from(translucent[0]) - 128).abs() <= 1
                        && translucent[1] == 255
                        && translucent[2] == 0,
                    "gamma UI after foreground: {translucent:?}"
                );
                assert!(
                    output
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|pixel| pixel[3] == 255),
                    "interpolation metadata is never coverage"
                );
            }
            assert_ne!(
                &real_pixels[32 * 256 * 4 + 32 * 4..32 * 256 * 4 + 60 * 4],
                &generated_pixels[32 * 256 * 4 + 32 * 4..32 * 256 * 4 + 60 * 4],
                "world interpolation must remain visible outside UI"
            );
            capture(
                &format!("real-{}x{}-{samples}x", render[0], render[1]),
                display,
                &real_pixels,
            );
            capture(
                &format!("generated-{}x{}-{samples}x", render[0], render[1]),
                display,
                &generated_pixels,
            );
        }
    }
}

#[test]
fn camera_history_rejects_reanchors_teleports_and_cuts_before_interpolation() {
    use crate::motion_blur::history::CameraHistory;
    let make =
        |pose, epoch| CameraHistory::new(Mat4::IDENTITY, pose, UVec4::new(0, 0, 256, 64), epoch);
    let previous = make(Mat4::IDENTITY, 0);
    assert!(previous.same_anchor(make(Mat4::from_translation(Vec3::X), 0)));
    assert!(
        !previous.same_anchor(make(Mat4::IDENTITY, 1)),
        "dimension or server-camera reanchor"
    );
    assert!(
        !previous.same_anchor(make(Mat4::from_translation(Vec3::X * 100.0), 0)),
        "teleport"
    );
    assert!(
        !previous.same_anchor(make(Mat4::from_rotation_y(std::f32::consts::PI), 0)),
        "camera cut"
    );
    assert!(!previous.same_anchor(make(Mat4::ZERO, 0)), "invalid camera");
}
