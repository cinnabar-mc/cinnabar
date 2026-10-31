// Adapted from Horizon under the MIT license; see THIRD_PARTY_NOTICES.md.

use super::*;

#[test]
fn shader_validates() {
    let module = naga::front::wgsl::parse_str(include_str!("optical_flow.wgsl")).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap();
}

/// A device with WebGPU's default limits (what every GPU wgpu runs on has), or `None`
/// without a GPU.
pub fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = match bevy::tasks::block_on(instance.request_adapter(&Default::default())) {
        Ok(adapter) if adapter.get_info().backend != wgpu::Backend::Noop => adapter,
        Ok(_) | Err(wgpu::RequestAdapterError::NotFound { .. }) => {
            eprintln!("skipping frame_generation: missing native GPU adapter fixture");
            return None;
        }
        Err(error) => panic!("frame generation adapter request failed: {error}"),
    };
    Some(
        bevy::tasks::block_on(adapter.request_device(&Default::default()))
            .expect("frame generation fixture device"),
    )
}

/// Noise in 4x4 pixel cells, moved by `shift` pixels: what was at `p` is at `p + shift`.
/// A `seed` of 100 or more gives cells of one primary each.
pub fn pattern(size: [u32; 2], seed: u32, shift: [i32; 2]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((size[0] * size[1] * 4) as usize);
    for y in 0..size[1] as i32 {
        for x in 0..size[0] as i32 {
            let (cx, cy) = ((x - shift[0]).div_euclid(4), (y - shift[1]).div_euclid(4));
            let mut h = (cx as u32).wrapping_mul(0x9e37_79b9)
                ^ (cy as u32).wrapping_mul(0x85eb_ca6b)
                ^ seed.wrapping_mul(0xc2b2_ae35);
            h ^= h >> 15;
            h = h.wrapping_mul(0x2c1b_3c6d);
            h ^= h >> 12;
            let v = (h & 0xff) as u8;
            if seed >= 100 {
                // Primaries: no two neighbours' colours point the same way.
                let mut c = [0, 0, 0, 255];
                c[(h >> 8) as usize % 3] = 128 + v / 2;
                pixels.extend_from_slice(&c);
            } else {
                pixels.extend_from_slice(&[v, v.wrapping_add(40), v / 2, 255]);
            }
        }
    }
    pixels
}

/// Uploads a synthetic SDR image for optical-flow tests.
pub fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: [u32; 2],
    pixels: &[u8],
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
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
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * 4),
            rows_per_image: None,
        },
        texture.size(),
    );
    texture.create_view(&Default::default())
}

/// `texture`'s texels (`texel` bytes each), row by row.
pub fn read(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    texel: u32,
) -> Vec<u8> {
    let size = texture.size();
    let row = (size.width * texel).next_multiple_of(256);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * size.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mapped = buffer.slice(..).get_mapped_range().unwrap();
    mapped
        .chunks(row as usize)
        .flat_map(|r| r[..(size.width * texel) as usize].to_vec())
        .collect()
}

/// Submits one flow update before the next frame changes its uniform values.
fn run(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    flow: &mut OpticalFlow,
    color: &wgpu::TextureView,
    reset: bool,
) {
    let mut encoder = device.create_command_encoder(&Default::default());
    flow.dispatch(device, queue, &mut encoder, color, reset);
    queue.submit([encoder.finish()]);
}

/// Reads the signed pixel displacements from the finest flow level.
fn vectors(device: &wgpu::Device, queue: &wgpu::Queue, flow: &OpticalFlow) -> Vec<[i32; 2]> {
    read(device, queue, flow.vectors.texture(), 8)
        .chunks(8)
        .map(|c| {
            [
                i32::from_le_bytes(c[0..4].try_into().unwrap()),
                i32::from_le_bytes(c[4..8].try_into().unwrap()),
            ]
        })
        .collect()
}

/// Reads complete small GPU reduction buffers after their queued compute dispatches.
fn read_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &wgpu::Buffer,
    size: u64,
) -> Vec<u8> {
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(source, 0, &staging, 0, size);
    queue.submit([encoder.finish()]);
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    staging.slice(..).get_mapped_range().unwrap().to_vec()
}

#[test]
fn cut_finalization_consumes_all_sums_and_retains_recent_cut_history() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut flow = OpticalFlow::new(&device, [512, 288]);
    for (sums, expected, history) in [
        ([400_000_u32, 800_000, 700_000], 0.4, 0),
        ([600_000, 800_000, 700_000], 0.6, 1),
        ([0, 0, 0], 0.0, 2),
    ] {
        queue.write_buffer(&flow.scd_temp, 0, bytemuck::cast_slice(&sums));
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&flow.pipelines.scd_finalize);
            pass.set_bind_group(0, &flow.scd_finalize, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        queue.submit([encoder.finish()]);
        let result = read_buffer(&device, &queue, &flow.scd_output, 12);
        let score = f32::from_le_bytes(result[..4].try_into().unwrap());
        assert!((score - expected).abs() < 0.00001);
        assert_eq!(
            u32::from_le_bytes(result[4..8].try_into().unwrap()),
            history
        );
        assert_eq!(
            read_buffer(&device, &queue, &flow.scd_temp, 12),
            vec![0; 12]
        );
    }
    // Reuse the same context to exercise the next ordinary accumulation after finalization.
    let color = upload(&device, &queue, [512, 288], &pattern([512, 288], 1, [0, 0]));
    run(&device, &queue, &mut flow, &color, true);
    assert_eq!(
        read_buffer(&device, &queue, &flow.scd_temp, 12),
        vec![0; 12]
    );
}

/// A frame moved right by 16 and down by 8: the blocks inside report where they were
/// (-16, -8), and nothing reads as a scene change. A much darker frame then does.
#[test]
fn finds_a_shift_and_a_scene_change() {
    let Some((device, queue)) = device() else {
        return;
    };
    let size = [512, 288];
    let still = upload(&device, &queue, size, &pattern(size, 1, [0, 0]));
    let moved = upload(&device, &queue, size, &pattern(size, 1, [16, 8]));
    // Scene changes are told by the luma's histogram: a much darker frame.
    let dark: Vec<u8> = pattern(size, 2, [0, 0]).iter().map(|v| v / 5).collect();
    let other = upload(&device, &queue, size, &dark);
    let mut flow = OpticalFlow::new(&device, size);
    // The first 6 frames after a reset are scene changes.
    run(&device, &queue, &mut flow, &still, true);
    for _ in 0..6 {
        run(&device, &queue, &mut flow, &still, false);
    }
    run(&device, &queue, &mut flow, &moved, false);
    let [w, h] = flow.flow_sizes[0];
    let found = vectors(&device, &queue, &flow);
    let inside: Vec<_> = (4..h - 4)
        .flat_map(|y| (4..w - 4).map(move |x| (x, y)))
        .map(|(x, y)| found[(y * w + x) as usize])
        .collect();
    let right = inside.iter().filter(|v| **v == [-16, -8]).count();
    assert!(
        right * 10 >= inside.len() * 9,
        "{right} of {} blocks: {:?}",
        inside.len(),
        &inside[..8]
    );
    let scd = |device: &wgpu::Device, queue: &wgpu::Queue, flow: &OpticalFlow| {
        let mut encoder = device.create_command_encoder(&Default::default());
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 12,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(flow.scene_change(), 0, &buffer, 0, 12);
        queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let bytes = buffer.slice(..).get_mapped_range().unwrap().to_vec();
        u32::from_le_bytes(bytes[4..8].try_into().unwrap())
    };
    assert_eq!(scd(&device, &queue, &flow) & 0xf, 0);
    run(&device, &queue, &mut flow, &other, false);
    assert_eq!(scd(&device, &queue, &flow) & 1, 1);
    assert!(vectors(&device, &queue, &flow).iter().all(|v| *v == [0, 0]));
}
