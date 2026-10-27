//! The terrain world material passes on the production cube shader: ordinary terrain skips a
//! bound block, the material's pass draws the guest output, the Bedrock fallback draws it as
//! its Bedrock material until the material is ready, and revocation restores Bedrock.
use crate::chunk_constants;
use crate::gpu_snapshot;
use crate::material_shader;
use crate::shader_source;
use crate::solid_terrain_raster::{lighting_words, pattern_texture};

use bevy::math::Vec3;
use gpu_snapshot::{Draw, Gpu, RasterState};
use meshing::{Face, PackedQuad};

const ORIGIN: [i32; 3] = [16, 64, -16];
/// The bound material's slot plus one.
const TAG: u8 = 1;
/// Where the fixture binds the world material group's frame uniform.
const FRAME_BINDING: u32 = 28;

/// A guest material that draws pure red, lit and fogged by the host.
const RED: &str = "fn shade(input: WorldMaterialInput) -> Surface { return Surface(vec3<f32>(param(input, 0u), 0.0, 0.0), 1.0, vec3<f32>(0.0)); }";

/// One block's six faces, tagged with `tag`.
fn block(tag: u8) -> Vec<PackedQuad> {
    Face::ALL
        .map(|face| PackedQuad::new([4, 4, 4], face, 1, 1, 0).with_world_material(tag))
        .to_vec()
}

/// The production cube module with `definitions`, and the world material library when one is
/// given, its group rebound into the fixture's single group.
fn source(definitions: &[&str], library: Option<&str>) -> String {
    let chunk = material_shader::source(&format!(
        "{}\n{}",
        include_str!("../../src/chunk.wesl"),
        library.unwrap_or_default()
    ));
    format!(
        "{}\n{}",
        shader_source::standalone(&chunk, definitions),
        RASTER_VERTEX.replace(
            "RASTER_INDICES",
            &chunk_constants::STATIC_QUAD_INDICES
                .map(|index| format!("{index}u"))
                .join(", ")
        ),
    )
    .replace("@group(1) @binding(0)", "@group(0) @binding(27)")
    .replace(
        "@group(2) @binding(0)",
        &format!("@group(0) @binding({FRAME_BINDING})"),
    )
}

/// Renders `quads` with the module `source` from a camera that sees every face.
fn render(gpu: &Gpu, quads: &[PackedQuad], source: &str, world: bool) -> Vec<u8> {
    render_with(gpu, quads, source, world.then_some([0.0; 4]))
}

/// [`render`], binding the world material frame with `effects` as its effects row when given.
fn render_with(
    gpu: &Gpu,
    quads: &[PackedQuad],
    source: &str,
    effects: Option<[f32; 4]>,
) -> Vec<u8> {
    let world = effects.is_some();
    let quad_words = quads.iter().flat_map(PackedQuad::words).collect::<Vec<_>>();
    let storage = wgpu::BufferUsages::STORAGE;
    let quad_buffer = gpu.words(&quad_words, storage);
    let origin = gpu.words(
        &[
            ORIGIN[0] as u32,
            ORIGIN[1] as u32,
            ORIGIN[2] as u32,
            0,
            0,
            0,
            0,
            0,
        ],
        storage,
    );
    let no_animation = u32::MAX;
    let materials = gpu.words(
        &[
            0,
            0,
            no_animation,
            0,
            0,
            0,
            assets::NO_MASK,
            assets::NO_MASK,
            0,
            assets::DEFAULT_AO_EXPONENT.to_bits(),
        ],
        storage,
    );
    let animations = gpu.words(&[0; 8], storage);
    let animation_frames = gpu.words(&[0; 4], storage);
    let clock = gpu.words(&[0; 4], wgpu::BufferUsages::UNIFORM);
    let streams = gpu.words(&lighting_words(quads.len()), storage);
    let records = gpu.buffer(&[0.0], storage);
    let tints = gpu.buffer(
        &[0.0; 8 + assets::seasonal_foliage::SEASONAL_FOLIAGE_COUNT * 4],
        storage,
    );
    let query_tables = gpu.words(
        &meshing::biome_lattice::query_table_words(),
        wgpu::BufferUsages::UNIFORM,
    );
    let mut atmosphere = [0.0; 32];
    atmosphere[19] = 1000.0;
    atmosphere[20] = 1001.0;
    let atmosphere = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    let table = render_model::LightmapInputs::default().build();
    let lightmap = gpu.lightmap(bytemuck::cast_slice(&table));
    let atlas = pattern_texture(gpu);
    let sampler = gpu.device.create_sampler(&Default::default());
    let frame = gpu.buffer(
        &mod_render::world_material::frame_uniform_with_effects(
            0.0,
            TAG,
            effects.unwrap_or_default(),
            &[1.0],
        ),
        wgpu::BufferUsages::UNIFORM,
    );
    let eye = Vec3::from_array(ORIGIN.map(|value| value as f32)) + Vec3::new(1.5, 7.5, 9.5);
    let target = Vec3::from_array(ORIGIN.map(|value| value as f32)) + Vec3::splat(4.5);
    let clip_from_world =
        glam::camera::rh::proj::directx::perspective_infinite_reverse(1.4, 1.0, 0.05)
            * glam::camera::rh::view::look_at_mat4(eye, target, Vec3::Y);
    let view = gpu.buffer(
        &gpu_snapshot::view(clip_from_world, eye),
        wgpu::BufferUsages::UNIFORM,
    );
    let mut bindings = vec![
        (0, view.as_entire_binding()),
        (1, quad_buffer.as_entire_binding()),
        (2, origin.as_entire_binding()),
        (3, materials.as_entire_binding()),
        (6, wgpu::BindingResource::Sampler(&sampler)),
        (7, records.as_entire_binding()),
        (8, tints.as_entire_binding()),
        (9, animations.as_entire_binding()),
        (10, animation_frames.as_entire_binding()),
        (11, clock.as_entire_binding()),
        (13, streams.as_entire_binding()),
        (15, atmosphere.as_entire_binding()),
        (
            material_shader::BIOME_QUERY_TABLES_BINDING,
            query_tables.as_entire_binding(),
        ),
        (27, lightmap.as_entire_binding()),
        (
            material_shader::NATIVE_LEAF_SAMPLER_BINDING,
            wgpu::BindingResource::Sampler(&sampler),
        ),
    ];
    for binding in material_shader::NATIVE_LEAF_TEXTURE_BINDINGS {
        bindings.push((binding, wgpu::BindingResource::TextureView(&atlas)));
    }
    if world {
        bindings.push((FRAME_BINDING, frame.as_entire_binding()));
    }
    let bindings = bindings
        .into_iter()
        .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource })
        .collect::<Vec<_>>();
    gpu.render_with_state(
        source,
        "raster_vertex",
        &[Draw {
            fragment: "fragment",
            vertices: 0..quads.len() as u32 * 6,
            bindings: &bindings,
            blend: None,
            write_depth: true,
        }],
        RasterState::default(),
    )
}

/// Pixels that differ from the frame's corner, which only background reaches.
fn covered(frame: &[u8]) -> Vec<[u8; 4]> {
    let background: [u8; 4] = frame[..4].try_into().unwrap();
    frame
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .filter(|pixel| *pixel != background)
        .collect()
}

#[test]
fn terrain_world_material_passes_draw_guest_fallback_and_restored_bedrock() {
    let Some(gpu) = Gpu::for_fixture("terrain world material raster") else {
        return;
    };
    let material = mod_render::world_material::compose(RED, 0).unwrap();
    let library = material.library(2);
    let bedrock = render(&gpu, &block(0), &source(&[], None), false);
    assert!(!covered(&bedrock).is_empty(), "the camera sees the block");
    // Ordinary terrain skips a bound block entirely.
    let skipped = render(&gpu, &block(TAG), &source(&[], None), false);
    assert!(
        covered(&skipped).is_empty(),
        "ordinary terrain draws no bound face"
    );
    // The material's pass draws the guest output, lit by the host.
    let world = render(
        &gpu,
        &block(TAG),
        &source(&["WORLD_MATERIAL", "WORLD_MATERIAL_OPAQUE"], Some(&library)),
        true,
    );
    let guest = covered(&world);
    assert!(!guest.is_empty());
    assert!(
        guest.iter().all(|pixel| pixel[1] < 8 && pixel[2] < 8),
        "the guest's red reaches every bound pixel: {:?}",
        &guest[..guest.len().min(4)]
    );
    assert!(guest.iter().any(|pixel| pixel[0] > 64));
    // Another material's pass draws nothing of this block.
    let other = render(
        &gpu,
        &block(TAG + 1),
        &source(&["WORLD_MATERIAL", "WORLD_MATERIAL_OPAQUE"], Some(&library)),
        true,
    );
    assert!(covered(&other).is_empty());
    // Until the material is ready, the fallback draws the bound block exactly as Bedrock.
    let fallback = render(
        &gpu,
        &block(TAG),
        &source(&["WORLD_MATERIAL_FALLBACK"], None),
        false,
    );
    assert_eq!(fallback, bedrock);
    // Revocation remeshes the block untagged, and ordinary terrain draws it as Bedrock again.
    let restored = render(&gpu, &block(0), &source(&[], None), false);
    assert_eq!(restored, bedrock);
    let fallback_untagged = render(
        &gpu,
        &block(0),
        &source(&["WORLD_MATERIAL_FALLBACK"], None),
        false,
    );
    assert!(
        covered(&fallback_untagged).is_empty(),
        "the fallback draws only bound faces"
    );
}

/// The acceptance view of a bound block: a cutout class drops what the guest leaves uncovered,
/// and emission brightens the face beyond what the host's lighting alone gives.
#[test]
fn bound_block_acceptance_follows_coverage_and_emission() {
    let Some(gpu) = Gpu::for_fixture("terrain world material acceptance") else {
        return;
    };
    let draw = |shade: &str, class: &str| {
        let material = mod_render::world_material::compose(shade, 0).unwrap();
        covered(&render(
            &gpu,
            &block(TAG),
            &source(&["WORLD_MATERIAL", class], Some(&material.library(2))),
            true,
        ))
    };
    let uncovered = draw(
        "fn shade(input: WorldMaterialInput) -> Surface { return Surface(vec3<f32>(1.0), 0.0, vec3<f32>(0.0)); }",
        "WORLD_MATERIAL_CUTOUT",
    );
    assert!(
        uncovered.is_empty(),
        "a cutout class discards uncovered pixels"
    );
    let dark = draw(
        "fn shade(input: WorldMaterialInput) -> Surface { return Surface(vec3<f32>(0.2), 1.0, vec3<f32>(0.0)); }",
        "WORLD_MATERIAL_OPAQUE",
    );
    let glowing = draw(
        "fn shade(input: WorldMaterialInput) -> Surface { return Surface(vec3<f32>(0.2), 1.0, vec3<f32>(0.6)); }",
        "WORLD_MATERIAL_OPAQUE",
    );
    assert_eq!(dark.len(), glowing.len());
    let sum = |pixels: &[[u8; 4]]| pixels.iter().map(|pixel| u32::from(pixel[0])).sum::<u32>();
    assert!(sum(&glowing) > sum(&dark), "emission adds after lighting");
}

/// The rows of the frame that `frame` covers, top and bottom.
fn covered_rows(frame: &[u8]) -> (usize, usize) {
    let background: [u8; 4] = frame[..4].try_into().unwrap();
    let side = gpu_snapshot::SNAPSHOT_SIDE as usize;
    let rows: Vec<usize> = (0..side)
        .filter(|row| {
            frame[row * side * 4..(row + 1) * side * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| *pixel != background)
        })
        .collect();
    (rows[0], *rows.last().unwrap())
}

/// A displacing material moves the bound block's drawn faces up within its declared bound, and
/// the host clamps a larger offset to it.
#[test]
fn a_displacing_material_moves_the_bound_block_within_its_bound() {
    let Some(gpu) = Gpu::for_fixture("terrain world material displacement") else {
        return;
    };
    let features = mod_render::world_material::Features {
        displace: true,
        ..Default::default()
    };
    let draw = |lift: f32, limit: f32| {
        let material = mod_render::world_material::compose_with(
            &format!(
                "{RED}\nfn displace(input: WorldVertexInput) -> vec3<f32> {{ return vec3<f32>(0.0, {lift:.3}, 0.0); }}"
            ),
            0,
            features,
        )
        .unwrap();
        render_with(
            &gpu,
            &block(TAG),
            &source(
                &[
                    "WORLD_MATERIAL",
                    "WORLD_MATERIAL_OPAQUE",
                    "WORLD_MATERIAL_DISPLACE",
                ],
                Some(&material.library(2)),
            ),
            Some([limit, 0.0, 0.0, 0.0]),
        )
    };
    let still = covered_rows(&draw(0.0, 1.0));
    let lifted = covered_rows(&draw(0.5, 1.0));
    assert!(
        lifted.0 < still.0 && lifted.1 < still.1,
        "{still:?} -> {lifted:?}"
    );
    assert_eq!(
        covered_rows(&draw(8.0, 0.5)),
        lifted,
        "clamped to the bound"
    );
}

const RASTER_VERTEX: &str = r#"
@vertex fn raster_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    var indices = array<u32, 6>(RASTER_INDICES);
    return cube_vertex(indices[index % 6u], index / 6u);
}
"#;
