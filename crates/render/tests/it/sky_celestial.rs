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

/// A `width` x `height` sRGB sprite with a transparent one-texel border around each `cell`-sized
/// cell, whose opaque interiors vary by cell and texel.
fn bordered_sprite(gpu: &Gpu, width: u32, height: u32, cell: u32) -> wgpu::TextureView {
    let mut texels = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let (cx, cy) = (x % cell, y % cell);
            let border = cx == 0 || cy == 0 || cx == cell - 1 || cy == cell - 1;
            let id = (y / cell) * (width / cell) + x / cell;
            texels.extend(if border {
                [0, 0, 0, 0]
            } else {
                [
                    (40 + id * 27 + x * 9) as u8,
                    (200 - id * 19 + y * 7) as u8,
                    (90 + (x * y) % 120) as u8,
                    (160 + id * 11) as u8,
                ]
            });
        }
    }
    gpu.device
        .create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("bordered celestial sprite"),
                size: wgpu::Extent3d {
                    width,
                    height,
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
            &texels,
        )
        .create_view(&Default::default())
}

/// The sky at `ticks` through a 90-degree view turned `offset` radians away from `towards`,
/// with bordered sun and moon sprites, or transparent ones without `sprites`.
fn sky_towards(
    gpu: &Gpu,
    source: &str,
    (ticks, towards, offset): (f64, fn(&AtmosphereFrame) -> Vec3, f32),
    sprites: bool,
) -> Vec<u8> {
    let frame = AtmosphereFrame::from_bedrock_time(ticks, 0.0, 0.0);
    let mut view = gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO);
    let rotation = Quat::from_rotation_arc(Vec3::Z, towards(&frame).normalize())
        * Quat::from_rotation_y(offset)
        * Quat::from_rotation_x(offset * 0.6);
    view[48..64].copy_from_slice(&Mat4::from_quat(rotation).to_cols_array());
    let view = gpu.buffer(&view, wgpu::BufferUsages::UNIFORM);
    let atmosphere = gpu.buffer(
        bytemuck::cast_slice(bytemuck::bytes_of(&frame)),
        wgpu::BufferUsages::UNIFORM,
    );
    let stars = gpu.buffer(&[0.0; 4], wgpu::BufferUsages::STORAGE);
    let (sun, moon) = if sprites {
        // Four by two phase cells, as the moon atlas.
        (
            bordered_sprite(gpu, 8, 8, 8),
            bordered_sprite(gpu, 24, 12, 6),
        )
    } else {
        (sprite(gpu, [0; 4]), sprite(gpu, [0; 4]))
    };
    let neutral = sprite(gpu, [255; 4]);
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
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
            resource: wgpu::BindingResource::TextureView(&moon),
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
        source,
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

/// Skipping the sprite outside its quad leaves every pixel as compositing `rgb * 0` did,
/// including filtered texels on either side of the quad edge and every moon phase cell.
#[test]
fn coverage_skip_matches_compositing_uncovered_sprites() {
    let Some(gpu) = Gpu::for_fixture("coverage_skip_matches_compositing_uncovered_sprites") else {
        return;
    };
    let shipped = include_str!("../../src/atmosphere.wesl");
    let skip = "    if (mapping.z == 0.0) { return vec4(0.0); }\n";
    assert_eq!(
        shipped.matches(skip).count(),
        2,
        "sun and moon skip uncovered pixels"
    );
    let composited = shader_source::standalone(&shipped.replace(skip, ""), &[]);
    let shipped = shader_source::standalone(shipped, &[]);
    assert_ne!(
        shipped, composited,
        "the comparison shader composites uncovered pixels"
    );
    let sun = |frame: &AtmosphereFrame| Vec3::from(frame.sun_direction());
    let moon = |frame: &AtmosphereFrame| -Vec3::from(frame.sun_direction());
    let mut views = Vec::new();
    for offset in [0.0, 0.12, 0.2, 0.26] {
        views.push((6_000.0, sun as fn(&AtmosphereFrame) -> Vec3, offset));
        for phase in 0..8 {
            views.push((18_000.0 + 24_000.0 * f64::from(phase), moon, offset));
        }
    }
    for view in views {
        let (ticks, _, offset) = view;
        let expected = sky_towards(&gpu, &composited, view, true);
        let actual = sky_towards(&gpu, &shipped, view, true);
        let differing = expected
            .chunks(4)
            .zip(actual.chunks(4))
            .filter(|(expected, actual)| expected != actual)
            .count();
        assert_eq!(differing, 0, "ticks {ticks}, offset {offset}");
        if offset == 0.0 {
            let centre = (SNAPSHOT_SIDE as usize / 2 * SNAPSHOT_SIDE as usize
                + SNAPSHOT_SIDE as usize / 2)
                * 4;
            let bare = sky_towards(&gpu, &shipped, view, false);
            assert_ne!(
                actual[centre..centre + 4],
                bare[centre..centre + 4],
                "ticks {ticks}: the sprite shows at the view centre"
            );
        }
    }
}
