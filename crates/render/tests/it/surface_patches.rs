//! Execute production patch coordinates and interpolation weights on adjacent retained tiles.
use crate::gpu_snapshot::{Draw, Gpu};

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
