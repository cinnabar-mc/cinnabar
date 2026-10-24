//! The world material showcase's guest shaders, rastered over a full-frame surface through the
//! host's own guest library and output bounding, each checked for the pixels that tell its
//! effect apart: the dissolve's burning edge, the hologram's rim, the energy flow's heat and
//! motion, the status tint and damage flash, and the portal's parallax.

use crate::gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE};

/// One showcase shader, read from the Experience's client part.
macro_rules! shader {
    ($name:literal) => {
        include_str!(concat!(
            "../../../../examples/experiences/material-fx/client/shaders/",
            $name,
            ".wgsl"
        ))
    };
}

/// What the host would hand `shade` at each pixel: the surface spans the frame with `uv` in
/// `[0, 1]`, the world position four blocks across it, the base colour and light given, and the
/// normal leaning from `-tilt` to `+tilt` along x towards a camera straight ahead.
struct Inputs {
    base: [f32; 4],
    phase: f32,
    tilt: f32,
    /// The view direction, which the portal looks through.
    view: [f32; 3],
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            base: [0.5, 0.5, 0.5, 1.0],
            phase: 0.0,
            tilt: 0.0,
            view: [0.0, 0.0, 1.0],
        }
    }
}

/// The full-frame vertex stage and a fragment that resolves the guest surface and writes its
/// colour plus emission with its coverage.
fn source(library: &str, inputs: &Inputs) -> String {
    let [r, g, b, a] = inputs.base;
    let [vx, vy, vz] = inputs.view;
    format!(
        "{library}
struct FxVertex {{ @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }}
@vertex fn fx_vertex(@builtin(vertex_index) index: u32) -> FxVertex {{
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return FxVertex(vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0), vec2<f32>(corner.x, 1.0 - corner.y));
}}
@fragment fn fx_fragment(vertex: FxVertex) -> @location(0) vec4<f32> {{
    let uv = clamp(vertex.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let lean = (uv.x * 2.0 - 1.0) * {tilt:?};
    let input = WorldMaterialInput(uv, vec3<f32>(uv.x * 4.0, uv.y * 4.0, 0.0),
        vec4<f32>({r:?}, {g:?}, {b:?}, {a:?}), vec4<f32>(1.0), vec4<f32>(0.0), vec4<f32>(0.0),
        {phase:?}, normalize(vec3<f32>(lean, 0.0, 1.0)), normalize(vec3<f32>({vx:?}, {vy:?}, {vz:?})));
    let surface = world_material_resolve(input);
    return vec4<f32>(surface.color + surface.emission, surface.coverage);
}}
",
        tilt = inputs.tilt,
        phase = inputs.phase,
    )
}

/// A `side`×`side` RGBA texture of `texel(x, y)`.
fn texture(gpu: &Gpu, side: u32, texel: impl Fn(u32, u32) -> [u8; 4]) -> wgpu::TextureView {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("material fx texture"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let texels: Vec<u8> = (0..side * side)
        .flat_map(|index| texel(index % side, index / side))
        .collect();
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(side * 4),
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

/// Rasters `shade` with `params`, its textures and `inputs`; returns RGBA8 rows.
fn raster(
    gpu: &Gpu,
    shade: &str,
    textures: &[wgpu::TextureView],
    params: &[f32],
    inputs: &Inputs,
) -> Vec<u8> {
    let material = mod_render::world_material::compose(shade, textures.len())
        .expect("the showcase shader passes the host's sandbox");
    let library = material.library(0);
    let frame = gpu.buffer(
        &mod_render::world_material::frame_uniform(0.0, 1, params),
        wgpu::BufferUsages::UNIFORM,
    );
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    let mut bindings = vec![wgpu::BindGroupEntry {
        binding: 0,
        resource: frame.as_entire_binding(),
    }];
    for (index, view) in textures.iter().enumerate() {
        bindings.push(wgpu::BindGroupEntry {
            binding: index as u32 + 1,
            resource: wgpu::BindingResource::TextureView(view),
        });
    }
    if !textures.is_empty() {
        bindings.push(wgpu::BindGroupEntry {
            binding: textures.len() as u32 + 1,
            resource: wgpu::BindingResource::Sampler(&sampler),
        });
    }
    gpu.render(
        &source(&library, inputs),
        "fx_vertex",
        &[Draw {
            fragment: "fx_fragment",
            vertices: 0..3,
            bindings: &bindings,
            blend: None,
            write_depth: false,
        }],
    )
}

/// The pixel at `(u, v)` of a frame, both in `[0, 1)`.
fn at(frame: &[u8], u: f32, v: f32) -> [u8; 4] {
    let side = SNAPSHOT_SIDE as f32;
    let (x, y) = ((u * side) as usize, (v * side) as usize);
    let index = (y * SNAPSHOT_SIDE as usize + x) * 4;
    frame[index..index + 4].try_into().unwrap()
}

/// Sum of a pixel's colour channels.
fn brightness(pixel: [u8; 4]) -> u32 {
    pixel[..3].iter().map(|&c| u32::from(c)).sum()
}

#[test]
fn the_dissolve_cuts_below_its_threshold_and_burns_just_above_it() {
    let Some(gpu) = Gpu::for_fixture("material fx dissolve") else {
        return;
    };
    // Edge width 0.1, orange edge, emission 6, the cut held at 0.5.
    let params = [0.1, 1.0, 0.42, 0.08, 6.0, 0.5];
    let draw = |noise: u8| {
        let noise = texture(&gpu, 1, |_, _| [noise, noise, noise, 255]);
        at(
            &raster(
                &gpu,
                shader!("dissolve"),
                &[noise],
                &params,
                &Inputs::default(),
            ),
            0.5,
            0.5,
        )
    };
    let gone = draw(77);
    assert_eq!(
        gone[3], 0,
        "noise below the cut leaves no coverage: {gone:?}"
    );
    let burning = draw(148);
    assert!(
        burning[3] == 255
            && burning[0] == 255
            && burning[0] > burning[1]
            && burning[1] > burning[2],
        "noise just above the cut burns orange: {burning:?}"
    );
    let whole = draw(243);
    assert!(
        whole[3] == 255
            && (i32::from(whole[0]) - i32::from(whole[2])).abs() < 4
            && brightness(whole) < brightness(burning),
        "noise far above the cut keeps the body's own colour: {whole:?}"
    );
}

#[test]
fn the_hologram_rim_outshines_its_face_and_its_scanlines_alternate() {
    let Some(gpu) = Gpu::for_fixture("material fx hologram") else {
        return;
    };
    let frame = raster(
        &gpu,
        shader!("hologram"),
        &[],
        &[0.25, 0.85, 1.0, 6.0],
        &Inputs {
            tilt: 6.0,
            ..Default::default()
        },
    );
    let (face, rim) = (at(&frame, 0.5, 0.5), at(&frame, 0.98, 0.5));
    assert!(brightness(rim) > brightness(face) + 120, "{rim:?} {face:?}");
    assert!(
        rim[2] > rim[0],
        "the rim glows in the hologram colour: {rim:?}"
    );
    let column: Vec<u32> = (0..64)
        .map(|row| brightness(at(&frame, 0.5, row as f32 / 64.0)))
        .collect();
    let (dim, bright) = (column.iter().min().unwrap(), column.iter().max().unwrap());
    assert!(
        bright > &(dim + 40),
        "scanlines alternate down the face: {column:?}"
    );
}

#[test]
fn the_energy_flow_glows_in_its_hot_veins_and_moves_with_the_phase() {
    let Some(gpu) = Gpu::for_fixture("material fx energy") else {
        return;
    };
    let params = [1.0, 3.5];
    let still = raster(&gpu, shader!("energy"), &[], &params, &Inputs::default());
    let pixels: Vec<[u8; 4]> = (0..32 * 32)
        .map(|index| {
            at(
                &still,
                (index % 32) as f32 / 32.0,
                (index / 32) as f32 / 32.0,
            )
        })
        .collect();
    let hottest = *pixels
        .iter()
        .max_by_key(|pixel| brightness(**pixel))
        .unwrap();
    let coolest = *pixels
        .iter()
        .min_by_key(|pixel| brightness(**pixel))
        .unwrap();
    assert!(
        hottest[0] == 255 && hottest[1] > 200,
        "hot veins glow white-hot: {hottest:?}"
    );
    assert!(
        coolest[0] < 90 && coolest[0] > coolest[1] && coolest[1] < 20,
        "cool rock stays dark crimson: {coolest:?}"
    );
    // A quarter of a flow cycle later the veins have moved.
    let later = raster(
        &gpu,
        shader!("energy"),
        &[],
        &params,
        &Inputs {
            phase: 1.0 / (1024.0 * 4.0),
            ..Default::default()
        },
    );
    let moved = (0..32 * 32)
        .filter(|index| {
            let (u, v) = ((index % 32) as f32 / 32.0, (index / 32) as f32 / 32.0);
            brightness(at(&still, u, v)).abs_diff(brightness(at(&later, u, v))) > 30
        })
        .count();
    assert!(moved > 100, "the flow animates with the phase: {moved}");
}

#[test]
fn the_status_tint_and_damage_flash_follow_their_parameters() {
    let Some(gpu) = Gpu::for_fixture("material fx status") else {
        return;
    };
    let draw = |params: &[f32]| {
        at(
            &raster(&gpu, shader!("status"), &[], params, &Inputs::default()),
            0.5,
            0.5,
        )
    };
    let plain = draw(&[0.0, 0.0, 0.0, 0.0, 0.0]);
    assert!(
        (i32::from(plain[0]) - i32::from(plain[1])).abs() < 4,
        "no status keeps the body grey: {plain:?}"
    );
    let poisoned = draw(&[0.35, 1.0, 0.2, 1.0, 0.0]);
    assert!(
        poisoned[1] > poisoned[0] + 40 && poisoned[1] > poisoned[2] + 40,
        "poison tints the body green: {poisoned:?}"
    );
    let flashed = draw(&[0.35, 1.0, 0.2, 1.0, 1.0]);
    assert!(
        flashed[..3].iter().all(|&c| c == 255),
        "a full flash washes it white: {flashed:?}"
    );
}

#[test]
fn the_portal_stars_shift_with_the_view_like_layers_at_depth() {
    let Some(gpu) = Gpu::for_fixture("material fx portal") else {
        return;
    };
    // One star in an otherwise black sky.
    let stars = || {
        texture(&gpu, 16, |x, y| {
            if (x, y) == (3, 5) {
                [255; 4]
            } else {
                [0, 0, 0, 255]
            }
        })
    };
    let params = [1.5, 0.45, 0.15, 0.85];
    let looking = |view: [f32; 3]| {
        raster(
            &gpu,
            shader!("portal"),
            &[stars()],
            &params,
            &Inputs {
                view,
                ..Default::default()
            },
        )
    };
    let ahead = looking([0.0, 0.0, 1.0]);
    let aside = looking([0.6, 0.0, 0.8]);
    let lit = |frame: &[u8]| {
        (0..64 * 64)
            .filter(|index| {
                let (u, v) = ((index % 64) as f32 / 64.0, (index / 64) as f32 / 64.0);
                at(frame, u, v)[0] > 200
            })
            .collect::<Vec<_>>()
    };
    let (stars_ahead, stars_aside) = (lit(&ahead), lit(&aside));
    assert!(!stars_ahead.is_empty(), "stars shine through the portal");
    assert_ne!(
        stars_ahead, stars_aside,
        "the star layers move with the view"
    );
    let darkest = (0..64 * 64)
        .map(|index| {
            brightness(at(
                &ahead,
                (index % 64) as f32 / 64.0,
                (index / 64) as f32 / 64.0,
            ))
        })
        .min()
        .unwrap();
    assert!(darkest < 120, "the sky between stars stays dark: {darkest}");
}
