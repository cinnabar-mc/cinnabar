// Adapted from Horizon under the MIT license; see THIRD_PARTY_NOTICES.md.

use super::super::optical_flow::{OpticalFlow, tests as of};
use super::*;

#[test]
fn shader_validates() {
    let module = naga::front::wgsl::parse_str(include_str!("interpolation.wgsl")).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap();
}

/// Decodes finite binary16 fixture pixels to f32 for error measurement.
fn f16(bits: u16) -> f32 {
    let (sign, exp, frac) = (bits >> 15, (bits >> 10) & 0x1f, bits & 0x3ff);
    let v = match exp {
        0 => f32::from(frac) / 1024.0 * 2f32.powi(-14),
        31 => f32::INFINITY,
        e => (1.0 + f32::from(frac) / 1024.0) * 2f32.powi(i32::from(e) - 15),
    };
    if sign == 1 { -v } else { v }
}

/// Runs `frames` (pattern shifts) through the optical flow and the interpolation with
/// a camera that moves by `reproject` each frame, and returns the mean error of the
/// last made frame's interior against `expected`, and the last two real frames'.
fn interpolate(
    seed: u32,
    shifts: &[[i32; 2]],
    reproject: Mat4,
    expected: [i32; 2],
    render_size: [u32; 2],
) -> Option<[f32; 3]> {
    let (device, queue) = of::device()?;
    let size = [512, 288];
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: render_size[0],
            height: render_size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let depth = depth.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(0.01),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    });
    queue.submit([encoder.finish()]);
    let mut flow = OpticalFlow::new(&device, size);
    let mut interpolation = Interpolation::new(&device, size);
    let frames: Vec<_> = shifts
        .iter()
        .map(|s| {
            let pixels = of::pattern(size, seed, *s);
            // COPY_SRC as the frame generation's world texture has.
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            queue.write_texture(
                t.as_image_copy(),
                &pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size[0] * 4),
                    rows_per_image: None,
                },
                t.size(),
            );
            (t.create_view(&Default::default()), pixels)
        })
        .collect();
    for (i, (color, _)) in frames.iter().enumerate() {
        let mut encoder = device.create_command_encoder(&Default::default());
        flow.dispatch(&device, &queue, &mut encoder, color, i == 0);
        interpolation.dispatch(
            &device,
            &queue,
            &mut encoder,
            &Inputs {
                color,
                depth: &depth,
                render_size,
                jitter: [0.0; 2],
                reproject: if i == 0 { Mat4::IDENTITY } else { reproject },
                near: 0.05,
                far: 512.0,
                fov_y: 1.2,
                reset: i == 0,
                flow_vectors: flow.vectors(),
                scene_change: flow.scene_change(),
            },
        );
        queue.submit([encoder.finish()]);
    }
    let out = of::read(&device, &queue, interpolation.output.texture(), 8);
    let made: Vec<f32> = out
        .chunks(2)
        .map(|c| f16(u16::from_le_bytes([c[0], c[1]])))
        .collect();
    let truth = of::pattern(size, seed, expected);
    let n = frames.len();
    let error = |pixels: &dyn Fn(usize) -> f32| {
        let (mut sum, mut count) = (0.0, 0.0);
        for y in 32..size[1] as usize - 32 {
            for x in 32..size[0] as usize - 32 {
                for c in 0..3 {
                    let i = (y * size[0] as usize + x) * 4 + c;
                    sum += (pixels(i) - f32::from(truth[i]) / 255.0).abs();
                    count += 1.0;
                }
            }
        }
        sum / count
    };
    Some([
        error(&|i| made[i]),
        error(&|i| f32::from(frames[n - 2].1[i]) / 255.0),
        error(&|i| f32::from(frames[n - 1].1[i]) / 255.0),
    ])
}

/// Something moving on its own (the camera still): the optical flow puts the made
/// frame's blocks halfway where its colours match better than the still camera's
/// (FSR prefers the game's vectors unless they clearly don't: primaries make it
/// clear). The first 10 frames after a reset use the game's vectors only.
#[test]
fn moves_what_the_optical_flow_follows_halfway() {
    let mut shifts = vec![[0, 0]; 20];
    shifts.push([16, 8]);
    let Some([made, previous, current]) =
        interpolate(100, &shifts, Mat4::IDENTITY, [8, 4], [512, 288])
    else {
        return;
    };
    // Where the still camera's two colours happen to be the same primary (a third of
    // the pixels), FSR keeps the game's vectors.
    assert!(
        made * 3.0 < previous.min(current),
        "{made} against {previous} and {current}"
    );
}

/// The camera turning: the game's motion vectors put the made frame halfway.
#[test]
fn moves_the_view_halfway_with_the_camera() {
    let size = [512.0, 288.0];
    // What is at a pixel now was 16 pixels to its right last frame.
    let reproject = Mat4::from_translation(glam::Vec3::new(32.0 / size[0], 0.0, 0.0));
    let shifts = [[0, 0], [-16, 0], [-32, 0], [-48, 0]];
    let Some([made, previous, current]) = interpolate(1, &shifts, reproject, [-40, 0], [512, 288])
    else {
        return;
    };
    assert!(
        made * 4.0 < previous.min(current),
        "{made} against {previous} and {current}"
    );
}

/// The optical-flow grid remains independent when depth is smaller than one flow block.
#[test]
fn moves_optical_flow_at_ten_percent_depth_resolution() {
    let mut shifts = vec![[0, 0]; 20];
    shifts.push([16, 8]);
    let Some([made, previous, current]) =
        interpolate(100, &shifts, Mat4::IDENTITY, [8, 4], [51, 29])
    else {
        return;
    };
    assert!(
        made * 2.0 < previous.min(current),
        "{made} against {previous} and {current}"
    );
}

#[test]
fn slow_object_motion_survives_native_resolution_normalization() {
    use wgpu::util::DeviceExt;
    let Some((device, queue)) = of::device() else {
        return;
    };
    for display in [[1920, 1080], [2560, 1440]] {
        let grid = display.map(|v: u32| v.div_ceil(super::super::optical_flow::BLOCK_SIZE));
        let params = Params {
            reproject: Mat4::IDENTITY.to_cols_array_2d(),
            device_to_view: [0.; 4],
            render_size: display.map(|v| v as i32),
            display_size: display.map(|v| v as i32),
            jitter: [0.; 2],
            optical_flow_scale: display.map(|v| 1. / v as f32),
            optical_flow_size: grid.map(|v| v as i32),
            reset: 0,
            tan_half_fov: 1.,
            level_size: [0; 2],
            _pad: [0; 2],
        };
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(include_str!("interpolation.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("optical_flow_vector_field_pass"),
            compilation_options: Default::default(),
            cache: None,
        });
        let flow = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: grid[0],
                height: grid[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg32Sint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let vectors = vec![[2_i32, 0]; (grid[0] * grid[1]) as usize];
        queue.write_texture(
            flow.as_image_copy(),
            bytemuck::cast_slice(&vectors),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(grid[0] * 8),
                rows_per_image: None,
            },
            flow.size(),
        );
        let color = of::upload(&device, &queue, [1, 1], &[255, 0, 0, 255]);
        let flow = flow.create_view(&Default::default());
        let fields = [0, 1].map(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: u64::from(grid[0] * grid[1]) * 4,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let binding = bind(
            &device,
            &pipeline,
            &[
                (0, whole(&params)),
                (1, wgpu::BindingResource::Sampler(&sampler)),
                (3, view(&color)),
                (4, view(&color)),
                (21, view(&flow)),
                (17, whole(&fields[0])),
                (18, whole(&fields[1])),
            ],
        );
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &binding, &[]);
            pass.dispatch_workgroups(grid[0].div_ceil(8), grid[1].div_ceil(8), 1);
        }
        let center = (grid[1] / 2) * grid[0] + grid[0] / 2;
        encoder.copy_buffer_to_buffer(&fields[0], u64::from(center) * 4, &staging, 0, 4);
        queue.submit([encoder.finish()]);
        staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, |result| result.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let mapped = staging.slice(..).get_mapped_range().unwrap();
        let packed = u32::from_le_bytes(mapped[..4].try_into().unwrap());
        assert_ne!(
            packed & PRIMARY_VECTOR_BIT_FOR_TEST,
            0,
            "{display:?}: missing flow vector"
        );
        let half_motion = f16((packed & 0xffff) as u16);
        assert!(
            (half_motion * display[0] as f32 - 1.).abs() < 0.02,
            "{display:?}: {half_motion}"
        );
    }
}

/// Packed source flag shared with the kernel; decode only the fields this fixture observes.
const PRIMARY_VECTOR_BIT_FOR_TEST: u32 = 1 << 31;
