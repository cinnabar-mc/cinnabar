//! A surface spanning the frame, as a mob's skin or a flat face sees it, and the wraith's and
//! hologram's checks.

use super::shader;
use crate::gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE};

/// What the host would hand `shade` at each pixel: the surface spans the frame with `uv` in
/// `[0, 1]` (v up), the world position `origin + uv.x * across + uv.y * up`, the base colour
/// and light given, and `normal` leaning from `-tilt` to `+tilt` along `across` towards the view.
pub(super) struct Inputs {
    pub base: [f32; 4],
    pub phase: f32,
    pub tilt: f32,
    pub view: [f32; 3],
    pub normal: [f32; 3],
    pub origin: [f32; 3],
    pub across: [f32; 3],
    pub up: [f32; 3],
    /// The fragment's linear view depth, which `fragment_depth` reads.
    pub depth: f32,
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            base: [0.5, 0.5, 0.5, 1.0],
            phase: 0.0,
            tilt: 0.0,
            view: [0.0, 0.0, 1.0],
            normal: [0.0, 0.0, 1.0],
            origin: [0.0; 3],
            across: [4.0, 0.0, 0.0],
            up: [0.0, 4.0, 0.0],
            depth: 1.0,
        }
    }
}

/// A WGSL `vec3<f32>` literal.
fn vec3(v: [f32; 3]) -> String {
    format!("vec3<f32>({:?}, {:?}, {:?})", v[0], v[1], v[2])
}

/// The full-frame vertex stage and a fragment that resolves the guest surface and writes its
/// colour plus emission with its coverage.
fn source(library: &str, inputs: &Inputs) -> String {
    let [r, g, b, a] = inputs.base;
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
    let across = {across};
    let world = {origin} + uv.x * across + uv.y * {up};
    world_material_set_fragment(vertex.position.xy, {depth:?});
    let input = WorldMaterialInput(uv, world,
        vec4<f32>({r:?}, {g:?}, {b:?}, {a:?}), vec4<f32>(1.0), vec4<f32>(0.0), vec4<f32>(0.0),
        {phase:?}, normalize({normal} + normalize(across) * lean), normalize({view}));
    let surface = world_material_resolve(input);
    return vec4<f32>(surface.color + surface.emission, surface.coverage);
}}
",
        tilt = inputs.tilt,
        phase = inputs.phase,
        depth = inputs.depth,
        across = vec3(inputs.across),
        origin = vec3(inputs.origin),
        up = vec3(inputs.up),
        normal = vec3(inputs.normal),
        view = vec3(inputs.view),
    )
}

/// A `side`×`side` RGBA texture of `texel(x, y)`.
pub(super) fn texture(
    gpu: &Gpu,
    side: u32,
    texel: impl Fn(u32, u32) -> [u8; 4],
) -> wgpu::TextureView {
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
        // Declared textures upload as sRGB, as the client uploads them.
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
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
pub(super) fn raster(
    gpu: &Gpu,
    shade: &str,
    textures: &[wgpu::TextureView],
    params: &[f32],
    inputs: &Inputs,
) -> Vec<u8> {
    raster_with(gpu, shade, textures, params, |library| {
        source(library, inputs)
    })
}

/// [`raster`] with effects.
pub(super) fn raster_effects(
    gpu: &Gpu,
    shade: &str,
    textures: &[wgpu::TextureView],
    params: &[f32],
    inputs: &Inputs,
    effects: &Effects<'_>,
) -> Vec<u8> {
    raster_with_effects(gpu, shade, textures, params, effects, |library| {
        source(library, inputs)
    })
}

/// What a material's effects read in the fixture: the functions its module has, the effects
/// row of its frame, the scene's colour and depth (the scene group rebound into the fixture's
/// group at [`SCENE_BINDINGS`]) and its cells.
#[derive(Default)]
pub(super) struct Effects<'a> {
    pub features: mod_render::world_material::Features,
    pub row: [f32; 4],
    pub scene: Option<(&'a wgpu::TextureView, &'a wgpu::TextureView)>,
    pub cells: Option<&'a wgpu::Buffer>,
}

/// Where the fixture binds the scene's colour and depth.
const SCENE_BINDINGS: [u32; 2] = [20, 21];

/// Rasters `shade` with `params` and its textures through the module `build` makes of the
/// composed guest library, whose `fx_vertex` and `fx_fragment` cover the frame.
pub(super) fn raster_with(
    gpu: &Gpu,
    shade: &str,
    textures: &[wgpu::TextureView],
    params: &[f32],
    build: impl FnOnce(&str) -> String,
) -> Vec<u8> {
    raster_with_effects(gpu, shade, textures, params, &Effects::default(), build)
}

/// [`raster_with`] with effects.
pub(super) fn raster_with_effects(
    gpu: &Gpu,
    shade: &str,
    textures: &[wgpu::TextureView],
    params: &[f32],
    effects: &Effects<'_>,
    build: impl FnOnce(&str) -> String,
) -> Vec<u8> {
    let material =
        mod_render::world_material::compose_with(shade, textures.len(), effects.features)
            .expect("the showcase shader passes the host's sandbox");
    let library = material
        .library(0)
        .replace(
            "@group(1) @binding(0)",
            &format!("@group(0) @binding({})", SCENE_BINDINGS[0]),
        )
        .replace(
            "@group(1) @binding(1)",
            &format!("@group(0) @binding({})", SCENE_BINDINGS[1]),
        );
    let frame = gpu.buffer(
        &mod_render::world_material::frame_uniform_with_effects(0.0, 1, effects.row, params),
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
    if let Some((colour, depth)) = effects.scene {
        for (binding, view) in SCENE_BINDINGS.into_iter().zip([colour, depth]) {
            bindings.push(wgpu::BindGroupEntry {
                binding,
                resource: wgpu::BindingResource::TextureView(view),
            });
        }
    }
    if let (Some(cells), Some(binding)) = (effects.cells, material.bindings().cells) {
        bindings.push(wgpu::BindGroupEntry {
            binding,
            resource: cells.as_entire_binding(),
        });
    }
    gpu.render(
        &build(&library),
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
pub(super) fn at(frame: &[u8], u: f32, v: f32) -> [u8; 4] {
    let side = SNAPSHOT_SIDE as f32;
    let (x, y) = ((u * side) as usize, (v * side) as usize);
    let index = (y * SNAPSHOT_SIDE as usize + x) * 4;
    frame[index..index + 4].try_into().unwrap()
}

/// Sum of a pixel's colour channels.
pub(super) fn brightness(pixel: [u8; 4]) -> u32 {
    pixel[..3].iter().map(|&c| u32::from(c)).sum()
}

#[test]
fn the_dissolve_cuts_below_its_threshold_and_burns_just_above_it() {
    let Some(gpu) = Gpu::for_fixture("material fx dissolve") else {
        return;
    };
    // Edge width 0.1, orange edge, emission 6, the cut held at 0.5.
    let params = [0.1, 1.0, 0.42, 0.08, 1.0, 0.5];
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
    // sRGB 190 decodes to 0.51, just above the cut.
    let burning = draw(190);
    assert!(
        burning[3] == 255 && burning[0] == 255 && burning[1] > burning[2] + 60,
        "noise just above the cut burns hot orange: {burning:?}"
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
