//! The bloom and outline passes over effects targets: an outline the target's width wide
//! around what is drawn and never over it, and bloom spreading its emission with distance.

use super::*;
use crate::scene_sampling::tests::fixture;

const SIDE: u32 = 64;
/// The drawn square: from this pixel to `SIDE - SQUARE` on both axes.
const SQUARE: u32 = 24;

/// Writes `texel` into `target`'s drawn texture over the square and zero elsewhere.
fn fill(queue: &wgpu::Queue, target: &Target, bytes: usize, texel: &[u8]) {
    let data: Vec<u8> = (0..SIDE * SIDE)
        .flat_map(|index| {
            let (x, y) = (index % SIDE, index / SIDE);
            let inside =
                (SQUARE..SIDE - SQUARE).contains(&x) && (SQUARE..SIDE - SQUARE).contains(&y);
            if inside {
                texel.to_vec()
            } else {
                vec![0; bytes]
            }
        })
        .collect();
    queue.write_texture(
        target._textures[0].as_image_copy(),
        &data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(SIDE * bytes as u32),
            rows_per_image: None,
        },
        Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        },
    );
}

/// A half-float's bits.
fn half(value: f32) -> [u8; 2] {
    // The values used here are exact in half precision.
    let bits = match value {
        0.0 => 0x0000_u16,
        1.0 => 0x3c00,
        4.0 => 0x4400,
        _ => panic!("{value}"),
    };
    bits.to_le_bytes()
}

#[test]
fn outlines_ring_what_is_drawn_and_bloom_spreads_its_emission() {
    let Some((device, queue, _)) = fixture() else {
        return;
    };
    let size = Extent3d {
        width: SIDE,
        height: SIDE,
        depth_or_array_layers: 1,
    };
    let format = TextureFormat::Rgba8Unorm;
    let render = |outline_width: f32, bloom_radius: f32, emissive: bool| {
        let mut view = EffectsView::new(&device, size, 1);
        view.ensure_composite(&device, format, 1);
        queue.write_buffer(
            &view.reach,
            0,
            bytemuck::cast_slice(&[bloom_radius, outline_width, 0.0, 0.0]),
        );
        // Red outline colour (sixteen over sixteen) and the width over eight.
        let width = (outline_width / 8.0 * 255.0).round() as u8;
        fill(&queue, &view.outline, 4, &[16, 0, 0, width]);
        let glow = if emissive { half(4.0) } else { half(0.0) };
        let one = half(1.0);
        fill(
            &queue,
            &view.emission,
            8,
            &[
                glow[0], glow[1], glow[0], glow[1], glow[0], glow[1], one[0], one[1],
            ],
        );
        let scene = device.create_texture(&TextureDescriptor {
            label: Some("scene"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let scene_view = scene.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &scene_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        }));
        view.encode_passes(
            &mut encoder,
            wgpu::RenderPassColorAttachment {
                view: &scene_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            },
            (format, 1),
        );
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(SIDE * SIDE * 4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            scene.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIDE * 4),
                    rows_per_image: None,
                },
            },
            size,
        );
        queue.submit([encoder.finish()]);
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .wgpu_device()
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        slice.get_mapped_range().unwrap().to_vec()
    };
    let pixel = |frame: &[u8], x: u32, y: u32| -> [u8; 4] {
        let at = ((y * SIDE + x) * 4) as usize;
        frame[at..at + 4].try_into().unwrap()
    };
    let middle = SIDE / 2;
    let outlined = render(2.0, 0.0, false);
    // Just outside the square the outline is red; three pixels out there is none.
    assert_eq!(pixel(&outlined, SQUARE - 1, middle)[0], 255);
    assert_eq!(pixel(&outlined, SQUARE - 2, middle)[0], 255);
    assert_eq!(pixel(&outlined, SQUARE - 3, middle), [0, 0, 0, 255]);
    // Inside the square nothing is drawn over the target.
    assert_eq!(pixel(&outlined, middle, middle), [0, 0, 0, 255]);
    // Around a corner the outline keeps its width as a distance, not a box.
    assert_eq!(pixel(&outlined, SQUARE - 2, SQUARE - 2), [0, 0, 0, 255]);
    assert_eq!(pixel(&outlined, SQUARE - 1, SQUARE - 1)[0], 255);
    // Bloom glows past the square, fading with distance, and adds to the square itself.
    let bloom = render(0.0, 8.0, true);
    let near = pixel(&bloom, SQUARE - 1, middle);
    let far = pixel(&bloom, SQUARE - 7, middle);
    assert!(near[0] > far[0] && far[0] > 0, "{near:?} {far:?}");
    assert!(near[0] == near[2], "white emission blooms white: {near:?}");
    assert_eq!(pixel(&bloom, 2, 2), [0, 0, 0, 255], "beyond its radius");
    assert!(pixel(&bloom, middle, middle)[0] > 200);
    // Without emission there is no bloom, and without width no outline.
    assert!(
        render(0.0, 8.0, false)
            .iter()
            .zip(render(0.0, 0.0, false).iter())
            .all(|(a, b)| a == b)
    );
    assert_eq!(
        pixel(&render(0.0, 0.0, false), SQUARE - 1, middle),
        [0, 0, 0, 255]
    );
}
