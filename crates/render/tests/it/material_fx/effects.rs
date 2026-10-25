//! World material effects through the host's guest library: a material reading the scene sees
//! the view's opaque colour where it is drawn, offset as it asks and clamped to the screen, and
//! its depth for soft edges; without the scene the same source reads nothing.

use super::flat::{Effects, Inputs, at, raster_effects};
use crate::gpu_snapshot::{Gpu, SNAPSHOT_SIDE};

/// A `SNAPSHOT_SIDE`-wide texture of `format` whose left half is `left` and right half
/// `right`, as RGBA8 or one 16-bit float channel.
fn halves(
    gpu: &Gpu,
    format: wgpu::TextureFormat,
    left: [u8; 8],
    right: [u8; 8],
) -> wgpu::TextureView {
    let side = SNAPSHOT_SIDE;
    let texel = format.block_copy_size(None).unwrap() as usize;
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scene fixture"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let bytes: Vec<u8> = (0..side * side)
        .flat_map(|index| {
            let half = if index % side < side / 2 { left } else { right };
            half[..texel].to_vec()
        })
        .collect();
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(side * texel as u32),
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&Default::default())
}

/// A half-float depth texel of `bits`.
fn depth(bits: u16) -> [u8; 8] {
    let [low, high] = bits.to_le_bytes();
    [low, high, 0, 0, 0, 0, 0, 0]
}

/// Half-float bits of 2 and 5.
const TWO: u16 = 0x4000;
const FIVE: u16 = 0x4500;

#[test]
fn a_scene_reading_material_sees_the_opaque_scene_offset_and_its_depth() {
    let Some(gpu) = Gpu::for_fixture("world material scene read") else {
        return;
    };
    // Red on the left half of the screen, blue on the right; the opaque surface two blocks
    // away on the left and five on the right.
    let colour = halves(
        &gpu,
        wgpu::TextureFormat::Rgba8Unorm,
        [255, 0, 0, 255, 0, 0, 0, 0],
        [0, 0, 255, 255, 0, 0, 0, 0],
    );
    let distance = halves(&gpu, wgpu::TextureFormat::R16Float, depth(TWO), depth(FIVE));
    let scene = Effects {
        features: mod_render::world_material::Features {
            scene: true,
            ..Default::default()
        },
        scene: Some((&colour, &distance)),
        ..Effects::default()
    };
    // Refraction: the colour `param 0` pixels to the right; green is how far in front of the
    // opaque surface the fragment is, over four blocks.
    let refract = "fn shade(input: WorldMaterialInput) -> Surface {
        let behind = scene_color(vec2<f32>(param(input, 0u), 0.0));
        let gap = clamp((scene_depth(vec2<f32>(0.0)) - fragment_depth()) / 4.0, 0.0, 1.0);
        return Surface(vec3<f32>(behind.r, gap, behind.b), 1.0, vec3<f32>(0.0));
    }";
    let inputs = Inputs {
        depth: 1.0,
        ..Inputs::default()
    };
    let side = SNAPSHOT_SIDE as f32;
    let straight = raster_effects(&gpu, refract, &[], &[0.0], &inputs, &scene);
    let (left, right) = (at(&straight, 0.4, 0.5), at(&straight, 0.6, 0.5));
    assert!(left[0] > 250 && left[2] < 5, "{left:?}");
    assert!(right[2] > 250 && right[0] < 5, "{right:?}");
    // One block in front of a surface two away on the left, four in front on the right.
    assert!((60..68).contains(&left[1]), "{left:?}");
    assert!(right[1] == 255, "{right:?}");
    // Bent by a fifth of the screen, the left of centre shows the right half.
    let bent = raster_effects(&gpu, refract, &[], &[side / 5.0], &inputs, &scene);
    assert!(at(&bent, 0.4, 0.5)[2] > 250);
    // Reads past the edge clamp to the screen.
    let far = raster_effects(&gpu, refract, &[], &[side * 10.0], &inputs, &scene);
    assert!(at(&far, 0.1, 0.5)[2] > 250);
    let nan = raster_effects(&gpu, refract, &[], &[f32::NAN], &inputs, &scene);
    assert_eq!(nan, straight, "a NaN offset reads the fragment's own pixel");
    // Without the scene the same source compiles and reads no colour and far depth.
    let without = raster_effects(&gpu, refract, &[], &[0.0], &inputs, &Effects::default());
    let pixel = at(&without, 0.4, 0.5);
    assert!(pixel[0] < 5 && pixel[2] < 5 && pixel[1] == 255, "{pixel:?}");
}
