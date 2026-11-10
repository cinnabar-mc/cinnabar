//! Execute production patch coordinates and interpolation weights on adjacent retained tiles.
use crate::{
    gpu_snapshot::{Draw, Gpu},
    shader_source,
};

#[test]
fn cube_and_cropped_model_faces_preserve_exponents_before_light_interpolation() {
    let Some(gpu) = Gpu::for_fixture("cube and cropped model AO exponents") else {
        return;
    };
    let production = include_str!("../../src/model.wesl");
    let coordinates = &production[production.find("fn surface_axes(").unwrap()
        ..production.find("/// Template-space position").unwrap()];
    let lighting = &production[production
        .find("    let ao = material_ambient_occlusion(")
        .unwrap()
        ..production.find("    out.visibility.y =").unwrap()];
    let cube = include_str!("../../src/chunk.wesl");
    let cube_lighting = &cube[cube
        .find("    let ao = material_ambient_occlusion(")
        .unwrap()..cube.find("    // Native RenderChunk").unwrap()];
    let source = format!(
        "{}\n{FIXTURE}\n{coordinates}\n{LIGHTING_FIXTURE}\n{lighting}\nreturn out;\n}}\n{CUBE_LIGHTING_FIXTURE}\n{cube_lighting}\nreturn out;\n}}\n{LIGHTING_WITNESS}",
        include_str!("../../src/material.wesl"),
    )
    .replace("SURFACE_TILE_MASK", &format!("{}u", meshing::surface::TILE_MASK))
    .replace("SURFACE_BORDER_REL_SHIFT", &format!("{}u", meshing::surface::BORDER_SHIFT - meshing::surface::TILE_SHIFT))
    .replace("EXPONENT_FLAGS", &format!("{}u", 80 << assets::MATERIAL_LEAF_AO_EXPONENT_SHIFT));
    for definitions in [&[][..], &["ENHANCED"][..]] {
        let shader = shader_source::standalone(&source, definitions);
        let pixels = gpu.render(
            &shader,
            "lighting_vertex",
            &[Draw {
                fragment: "lighting_fragment",
                vertices: 0..3,
                bindings: &[],
                blend: None,
                write_depth: false,
            }],
        );
        assert_eq!(
            &pixels[(128 * 256 + 128) * 4..][..4],
            &[0, 255, 0, 255],
            "{definitions:?}"
        );
    }
}

#[test]
fn adjacent_surface_tiles_interpolate_one_continuous_light_gradient() {
    let Some(gpu) = Gpu::for_fixture("terrain surface patch lighting") else {
        return;
    };
    let production = include_str!("../../src/model.wesl");
    let start = production.find("fn surface_axes(").unwrap();
    let end = production[start..]
        .find("/// Template-space position")
        .unwrap()
        + start;
    let shader = format!("{}\n{}\n{}", FIXTURE, &production[start..end], WITNESS)
        .replace(
            "SURFACE_TILE_MASK",
            &format!("{}u", meshing::surface::TILE_MASK),
        )
        .replace(
            "SURFACE_BORDER_REL_SHIFT",
            &format!(
                "{}u",
                meshing::surface::BORDER_SHIFT - meshing::surface::TILE_SHIFT
            ),
        );
    let pixels = gpu.render(
        &shader,
        "witness_vertex",
        &[Draw {
            fragment: "witness_fragment",
            vertices: 0..3,
            bindings: &[],
            blend: None,
            write_depth: false,
        }],
    );
    assert_eq!(&pixels[(128 * 256 + 128) * 4..][..4], &[0, 255, 0, 255]);
}

const FIXTURE: &str = r#"
struct ModelQuad { surface_patch: u32, quad_flags: u32 }
fn raw_model_position(quad: ModelQuad, corner: u32) -> vec3<f32> {
    let corners = array(vec3(0.0, 0.0, 0.0), vec3(1.0, 0.0, 0.0),
        vec3(1.0, 1.0, 0.0), vec3(0.0, 1.0, 0.0));
    return corners[corner];
}
"#;

const WITNESS: &str = r#"
struct Witness { @builtin(position) position: vec4<f32> }
@vertex fn witness_vertex(@builtin(vertex_index) index: u32) -> Witness {
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return Witness(vec4(uv * 2.0 - vec2(1.0), 0.5, 1.0));
}
@fragment fn witness_fragment() -> @location(0) vec4<f32> {
    // An independently specified affine light field: 2 + 4*x + 8*y.
    let samples = array(2.0, 6.0, 14.0, 10.0);
    var valid = true;
    for (var tile = 0u; tile < 9u; tile += 1u) {
        let quad = ModelQuad(tile | (16u << 4u), 5u);
        for (var corner = 0u; corner < 4u; corner += 1u) {
            let point = patch_coordinate(quad, corner);
            var light = 0.0;
            var sum = 0.0;
            for (var source = 0u; source < 4u; source += 1u) {
                let weight = patch_weight(quad, source, point);
                light += weight * samples[source];
                sum += weight;
            }
            valid = valid && abs(sum - 1.0) < 0.00001;
            valid = valid && abs(light - (2.0 + 4.0 * point.x + 8.0 * point.y)) < 0.00001;
        }
    }
    // The neighbouring edge and corner patches share these exact physical corners.
    valid = valid && all(patch_coordinate(ModelQuad(16u << 4u, 5u), 1u)
        == patch_coordinate(ModelQuad(1u | (16u << 4u), 5u), 0u));
    return select(vec4(1.0, 0.0, 0.0, 1.0), vec4(0.0, 1.0, 0.0, 1.0), valid);
}
"#;

const LIGHTING_FIXTURE: &str = r#"
struct LightingOutput {
    normal: vec3<f32>, lighting: vec3<f32>, sky_light: f32,
    ambient_occlusion: f32, native_light_levels: vec2<f32>, native_ao_face: f32,
}
/// Selects constant samples or four distinct corner samples on the same face.
fn model_light_sample(quad: ModelQuad, corner: u32) -> u32 {
    return select(corner, 2u, (quad.quad_flags & 256u) != 0u) << 8u;
}
/// Supplies independently chosen corner occlusion values.
fn light_ao_factor(value: u32) -> f32 { return array(0.2, 0.4, 0.6, 0.9)[value]; }
/// Leaves sample occlusion enabled for the exponent witness.
fn material_ambient_occlusion(ao: f32, flags: u32) -> f32 { return ao; }
/// Retains the supplied normal for this axis-aligned face.
fn model_shade_normal(normal: vec3<f32>, sample: u32) -> vec3<f32> { return normal; }
/// Supplies a non-unit face coefficient so it must be exponentiated too.
fn material_face_shade(normal: vec3<f32>, flat: bool, flags: u32) -> f32 { return 0.8; }
/// Keeps RGB lighting neutral while the production code shades it.
fn light_colour(sample: u32) -> vec3<f32> { return vec3(1.0); }
/// Keeps sky light constant while the production code crops it.
fn sky_illumination(sample: u32) -> f32 { return 1.0; }
/// Keeps native light coordinates constant while the production code crops them.
fn terrain_light_levels(sample: u32) -> vec2<f32> { return vec2(1.0); }
/// Runs the production lighting block with controlled face samples.
fn patch_lighting(quad: ModelQuad, corner: u32, flags: u32) -> LightingOutput {
    let material = MaterialGpu(0u, flags, 0u, 0u, 0u, 0u);
    let light_sample = model_light_sample(quad, corner);
    let is_lily_pad = false;
    let pad_shade = 1.0;
    var out: LightingOutput;
    out.normal = vec3(0.0, 0.0, -1.0);
"#;

const LIGHTING_WITNESS: &str = r#"
@vertex fn lighting_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(uv * 2.0 - vec2(1.0), 0.5, 1.0);
}
@fragment fn lighting_fragment() -> @location(0) vec4<f32> {
    var valid = true;
    for (var constant_samples = 0u; constant_samples < 2u; constant_samples += 1u) {
        for (var encoded = 0u; encoded < 2u; encoded += 1u) {
            let flags = select(0u, EXPONENT_FLAGS, encoded != 0u);
            let face = ModelQuad(0u, 5u | (constant_samples << 8u));
            let a = patch_lighting(face, 0u, flags).lighting.x;
            let b = patch_lighting(face, 1u, flags).lighting.x;
            let c = patch_lighting(face, 2u, flags).lighting.x;
            let d = patch_lighting(face, 3u, flags).lighting.x;
            @if(ENHANCED)
            for (var corner = 0u; corner < 4u; corner += 1u) {
                let sample = model_light_sample(face, corner);
                let ao = array(0.2, 0.4, 0.6, 0.9)[sample >> 8u];
                let expected = pow(ao * 0.8, select(1.0, 0.8, encoded != 0u));
                let actual = cube_lighting(sample, flags);
                valid = valid && all(abs(actual.lighting - vec3(expected)) < vec3(0.00001));
                valid = valid && abs(actual.lighting.x - array(a, b, c, d)[corner]) < 0.00001;
                valid = valid && abs(actual.ambient_occlusion - ao) < 0.00001;
            }
            for (var tile = 0u; tile < 9u; tile += 1u) {
                let patch = ModelQuad(tile | (16u << 4u), face.quad_flags);
                for (var corner = 0u; corner < 4u; corner += 1u) {
                    let point = patch_coordinate(patch, corner);
                    let expected = mix(mix(a, b, point.x), mix(d, c, point.x), point.y);
                    let actual = patch_lighting(patch, corner, flags);
                    valid = valid && all(abs(actual.lighting - vec3(expected)) < vec3(0.00001));
                    @if(!ENHANCED)
                    valid = valid && abs(actual.native_ao_face - expected) < 0.00001;
                }
            }
        }
    }
    return select(vec4(1.0, 0.0, 0.0, 1.0), vec4(0.0, 1.0, 0.0, 1.0), valid);
}
"#;

const CUBE_LIGHTING_FIXTURE: &str = r#"
/// Supplies the same side normal as the model lighting witness.
fn face_normal(face: u32) -> vec3<f32> { return vec3(0.0, 0.0, -1.0); }
/// Runs the production cube lighting block with controlled face samples.
fn cube_lighting(light_sample: u32, flags: u32) -> LightingOutput {
    let material = MaterialGpu(0u, flags, 0u, 0u, 0u, 0u);
    let face = 4u;
    var out: LightingOutput;
"#;
