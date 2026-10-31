use super::*;
use optical_flow::tests::{device, pattern, read};

#[test]
fn sizes_cover_native_and_reduced_world_depth_without_imposing_a_shared_stride() {
    let limits = wgpu::Limits::default();
    for display in [[1920, 1080], [2560, 1440], [517, 291]] {
        for render in [display, display.map(|v| (v / 10).max(1))] {
            assert!(
                FrameGenerationSize { display, render }
                    .validate(&limits)
                    .is_ok()
            );
        }
    }
}

#[test]
fn unsupported_sizes_are_rejected_before_gpu_allocation() {
    let limits = wgpu::Limits::default();
    for size in [
        FrameGenerationSize {
            display: [0, 1080],
            render: [1920, 1080],
        },
        FrameGenerationSize {
            display: [255, 1080],
            render: [255, 1080],
        },
        FrameGenerationSize {
            display: [1920, 63],
            render: [1920, 63],
        },
        FrameGenerationSize {
            display: [1920, 1080],
            render: [0, 1080],
        },
        FrameGenerationSize {
            display: [limits.max_texture_dimension_2d + 1, 1080],
            render: [1920, 1080],
        },
        FrameGenerationSize {
            display: [1920, 1080],
            render: [8192, 8192],
        },
    ] {
        assert!(size.validate(&limits).is_err(), "{size:?}");
    }
}

/// Creates one complete single-sampled texture for the public backend contract.
fn texture(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("frame generation contract fixture"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | if format == wgpu::TextureFormat::Depth32Float {
                wgpu::TextureUsages::RENDER_ATTACHMENT
            } else {
                wgpu::TextureUsages::COPY_DST
            },
        view_formats: &[],
    })
}

/// Uploads an original synthetic world image and clears its matching finite depth.
fn upload(queue: &wgpu::Queue, color: &wgpu::Texture, pixels: &[u8]) {
    queue.write_texture(
        color.as_image_copy(),
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(color.width() * 4),
            rows_per_image: None,
        },
        color.size(),
    );
}

/// Creates a continuous unjittered camera input borrowing the fixture's world textures.
fn input<'a>(color: &'a wgpu::Texture, depth: &'a wgpu::Texture) -> FrameGenerationInput<'a> {
    FrameGenerationInput {
        color,
        depth,
        previous_clip_from_clip: Mat4::IDENTITY,
        jitter: [0.0; 2],
        near: 0.05,
        far: 512.0,
        fov_y: 1.2,
        reset: false,
    }
}

/// Asserts RGB pixels are a current-frame copy within binary16 rounding error.
fn assert_real_image(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image: &wgpu::TextureView,
    pixels: &[u8],
) {
    let actual = read(device, queue, image.texture(), 8);
    for (actual, expected) in actual
        .as_chunks::<8>()
        .0
        .iter()
        .zip(pixels.as_chunks::<4>().0)
    {
        for channel in 0..3 {
            let value = half(u16::from_le_bytes([
                actual[channel * 2],
                actual[channel * 2 + 1],
            ]));
            assert!((value - f32::from(expected[channel]) / 255.0).abs() < 0.001);
        }
    }
}

/// Decodes nonnegative fixture binary16 values without introducing another dependency.
fn half(bits: u16) -> f32 {
    let exponent = (bits >> 10) & 31;
    let mantissa = bits & 1023;
    match exponent {
        0 => f32::from(mantissa) * 2_f32.powi(-24),
        31 => f32::INFINITY,
        value => (1.0 + f32::from(mantissa) / 1024.0) * 2_f32.powi(i32::from(value) - 15),
    }
}

#[test]
fn first_cut_and_invalidated_frames_never_reuse_an_old_world_image() {
    let Some((device, queue)) = device() else {
        return;
    };
    let size = FrameGenerationSize {
        display: [517, 291],
        render: [52, 29],
    };
    let color = texture(&device, size.display, wgpu::TextureFormat::Rgba8Unorm);
    let depth = texture(&device, size.render, wgpu::TextureFormat::Depth32Float);
    let mut generator = FrameGenerator::prepare(&device, size).unwrap();
    for (seed, explicit, invalidated) in [(1, false, false), (2, true, false), (3, false, true)] {
        let pixels = pattern(size.display, seed, [0, 0]);
        upload(&queue, &color, &pixels);
        if invalidated {
            generator.invalidate();
        }
        let mut frame = input(&color, &depth);
        frame.reset = explicit;
        let generated = generator.generate(&device, &queue, &frame).unwrap();
        assert!(generated.history_reset);
        assert_eq!(
            [
                generated.image.texture().width(),
                generated.image.texture().height()
            ],
            size.display
        );
        assert_real_image(&device, &queue, generated.image, &pixels);
    }
}

#[test]
fn malformed_input_invalidates_history_and_rejects_before_dispatch() {
    let Some((device, queue)) = device() else {
        return;
    };
    let size = FrameGenerationSize {
        display: [512, 288],
        render: [512, 288],
    };
    let color = texture(&device, size.display, wgpu::TextureFormat::Rgba8Unorm);
    let depth = texture(&device, size.render, wgpu::TextureFormat::Depth32Float);
    let mut generator = FrameGenerator::prepare(&device, size).unwrap();
    let mut frame = input(&color, &depth);
    generator.generate(&device, &queue, &frame).unwrap();
    assert!(
        !generator
            .generate(&device, &queue, &frame)
            .unwrap()
            .history_reset
    );
    for invalid in [f32::NAN, 0.0, -1.0, f32::INFINITY] {
        frame.near = invalid;
        assert!(generator.generate(&device, &queue, &frame).is_err());
        frame.near = 0.05;
        assert!(
            generator
                .generate(&device, &queue, &frame)
                .unwrap()
                .history_reset
        );
    }
    frame.previous_clip_from_clip = Mat4::ZERO;
    assert!(generator.generate(&device, &queue, &frame).is_err());
    let wrong_color = texture(&device, size.display, wgpu::TextureFormat::Rgba8UnormSrgb);
    assert!(
        generator
            .generate(&device, &queue, &input(&wrong_color, &depth))
            .is_err()
    );
    let wrong_depth = texture(&device, [32, 32], wgpu::TextureFormat::Depth32Float);
    assert!(
        generator
            .generate(&device, &queue, &input(&color, &wrong_depth))
            .is_err()
    );
}

#[test]
fn samples_on_or_behind_the_previous_camera_plane_use_current_colour() {
    let Some((device, queue)) = device() else {
        return;
    };
    let size = FrameGenerationSize {
        display: [512, 288],
        render: [512, 288],
    };
    let color = texture(&device, size.display, wgpu::TextureFormat::Rgba8Unorm);
    let depth = texture(&device, size.render, wgpu::TextureFormat::Depth32Float);
    let depth_view = depth.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(0.01),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    });
    queue.submit([encoder.finish()]);
    let mut generator = FrameGenerator::prepare(&device, size).unwrap();
    let previous = pattern(size.display, 1, [0, 0]);
    upload(&queue, &color, &previous);
    for _ in 0..12 {
        generator
            .generate(&device, &queue, &input(&color, &depth))
            .unwrap();
    }
    for (z_w, w_w) in [(1.0, -0.01), (0.0, -1.0)] {
        let current = pattern(size.display, 1, [16, 8]);
        upload(&queue, &color, &current);
        let mut frame = input(&color, &depth);
        frame.previous_clip_from_clip.z_axis.w = z_w;
        frame.previous_clip_from_clip.w_axis.w = w_w;
        let generated = generator.generate(&device, &queue, &frame).unwrap();
        assert!(!generated.history_reset);
        assert_real_image(&device, &queue, generated.image, &current);
    }
}

#[test]
fn complete_pipeline_bindings_validate_without_a_native_gpu_fixture() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let size = FrameGenerationSize {
        display: [512, 288],
        render: [51, 29],
    };
    let color = texture(&device, size.display, wgpu::TextureFormat::Rgba8Unorm);
    let depth = texture(&device, size.render, wgpu::TextureFormat::Depth32Float);
    let mut generator = FrameGenerator::prepare(&device, size).unwrap();
    let frame = input(&color, &depth);
    assert!(
        generator
            .generate(&device, &queue, &frame)
            .unwrap()
            .history_reset
    );
    assert!(
        !generator
            .generate(&device, &queue, &frame)
            .unwrap()
            .history_reset
    );
    generator.invalidate();
    assert!(
        generator
            .generate(&device, &queue, &frame)
            .unwrap()
            .history_reset
    );
    device.poll(wgpu::PollType::Poll).unwrap();
}
