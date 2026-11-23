//! The overworld sky composites its sun sprite only where the sprite's quad covers the pixel.
use crate::{gpu_snapshot, shader_source};
use bevy::math::{Mat4, Quat, Vec3};
use gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE};
use render_scene::AtmosphereFrame;
use wgpu::util::DeviceExt;

/// A one-texel sRGB sprite.
fn sprite(gpu: &Gpu, texel: [u8; 4]) -> wgpu::TextureView {
    gpu.device
        .create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("celestial coverage sprite"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &texel,
        )
        .create_view(&Default::default())
}

/// Renders the stock noon sky through a 90-degree view centred on the sun.
fn noon_sky_facing_sun(gpu: &Gpu, sun: [u8; 4]) -> Vec<u8> {
    let frame = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0);
    let mut view = gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO);
    // `View::world_from_view` is the fourth matrix; view rays leave along +Z.
    let towards_sun =
        Quat::from_rotation_arc(Vec3::Z, Vec3::from(frame.sun_direction()).normalize());
    view[48..64].copy_from_slice(&Mat4::from_quat(towards_sun).to_cols_array());
    let view = gpu.buffer(&view, wgpu::BufferUsages::UNIFORM);
    let atmosphere = gpu.buffer(
        bytemuck::cast_slice(bytemuck::bytes_of(&frame)),
        wgpu::BufferUsages::UNIFORM,
    );
    let stars = gpu.buffer(&[0.0; 4], wgpu::BufferUsages::STORAGE);
    let sun = sprite(gpu, sun);
    let neutral = sprite(gpu, [255; 4]);
    let sampler = gpu.device.create_sampler(&Default::default());
    let entries = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: atmosphere.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: wgpu::BindingResource::TextureView(&sun),
        },
        wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::TextureView(&neutral),
        },
        wgpu::BindGroupEntry {
            binding: 4,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
            binding: 5,
            resource: wgpu::BindingResource::TextureView(&neutral),
        },
        wgpu::BindGroupEntry {
            binding: 6,
            resource: stars.as_entire_binding(),
        },
    ];
    gpu.render_srgb(
        &shader_source::standalone(include_str!("../../src/atmosphere.wesl"), &[]),
        "atmosphere_vertex",
        &[Draw {
            fragment: "atmosphere_fragment",
            vertices: 0..3,
            bindings: &entries,
            blend: None,
            write_depth: false,
        }],
    )
}

#[test]
fn sun_texels_change_only_pixels_inside_the_sun_quad() {
    let Some(gpu) = Gpu::for_fixture("sun_texels_change_only_pixels_inside_the_sun_quad") else {
        return;
    };
    let bright = noon_sky_facing_sun(&gpu, [255, 255, 255, 255]);
    let dark = noon_sky_facing_sun(&gpu, [0, 0, 0, 0]);
    let side = SNAPSHOT_SIDE as usize;
    let centre = (side / 2 * side + side / 2) * 4;
    assert_ne!(
        bright[centre..centre + 3],
        dark[centre..centre + 3],
        "the sun sprite composites over the sky at its centre"
    );
    // The quad's half extent is tan(14.04 degrees), a quarter of the view's half width; the
    // margin admits any rotation of the quad about the view axis.
    let inside = |coordinate: usize| coordinate.abs_diff(side / 2) <= side / 5;
    let mut outside = 0;
    for y in 0..side {
        for x in 0..side {
            if inside(x) && inside(y) {
                continue;
            }
            outside += 1;
            let index = (y * side + x) * 4;
            assert_eq!(
                bright[index..index + 4],
                dark[index..index + 4],
                "sky pixel ({x}, {y}) outside the sun quad depends on its texels"
            );
        }
    }
    assert!(outside > side * side / 2);
}
