//! Execute the model shader's quarter turns, x turns included, against the mesher's: the turns that
//! put a cell overlay on each face must move template corners where the CPU bakes their lighting.
use crate::gpu_snapshot;

use gpu_snapshot::{Draw, Gpu};

/// The production turn helpers, from `rotate_cross` up to the next entry point or directive, and
/// the witness.
fn shader() -> String {
    let source = include_str!("../../src/model.wesl");
    let functions = source.split_once("fn rotate_cross(").unwrap().1;
    let end = ["\n#", "\n@"]
        .into_iter()
        .filter_map(|marker| functions.find(marker))
        .min()
        .unwrap();
    format!("fn rotate_cross({}\n{WITNESS}", &functions[..end])
}

#[test]
fn production_model_turn_shader_is_valid() {
    let source = shader();
    assert!(source.contains("fn rotate_model_turn("));
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
}

#[test]
fn every_overlay_face_turn_moves_corners_where_the_mesher_does() {
    let Some(gpu) = Gpu::for_fixture("every_overlay_face_turn_moves_corners_where_the_mesher_does")
    else {
        return;
    };
    let source = shader();
    for face in 0..6 {
        let turn = meshing::overlay_model_turn(face).unwrap();
        for corner in [[0, 0, 0], [256, 0, 32], [16, 240, 128], [256, 256, 256]] {
            let expected = meshing::turn_model_position(corner, turn).map(i32::from);
            let [x, y, z] = corner.map(i32::from);
            // Three vec4<i32>: the corner, the mesher's turned corner, and the turn.
            let words = [
                x,
                y,
                z,
                0,
                expected[0],
                expected[1],
                expected[2],
                0,
                turn as i32,
                0,
                0,
                0,
            ];
            let buffer = gpu.buffer(bytemuck::cast_slice(&words), wgpu::BufferUsages::UNIFORM);
            let pixels = gpu.render(
                &source,
                "witness_vertex",
                &[Draw {
                    fragment: "witness_fragment",
                    vertices: 0..3,
                    bindings: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    }],
                    blend: None,
                    write_depth: false,
                }],
            );
            assert_eq!(
                &pixels[(128 * 256 + 128) * 4..][..4],
                &[0, 255, 0, 255],
                "face {face} corner {corner:?}"
            );
        }
    }
}

const WITNESS: &str = r#"
struct TurnCase { corner: vec4<i32>, expected: vec4<i32>, turn: vec4<i32> }
@group(0) @binding(0) var<uniform> turn_case: TurnCase;
struct WitnessOutput { @builtin(position) position: vec4<f32>, }
@vertex fn witness_vertex(@builtin(vertex_index) index: u32) -> WitnessOutput {
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return WitnessOutput(vec4(uv * 2.0 - vec2(1.0), 0.5, 1.0));
}
@fragment fn witness_fragment() -> @location(0) vec4<f32> {
    let actual = rotate_model_turn(vec3<f32>(turn_case.corner.xyz) / 256.0, u32(turn_case.turn.x));
    let expected = vec3<f32>(turn_case.expected.xyz) / 256.0;
    return select(vec4(1., 0., 0., 1.), vec4(0., 1., 0., 1.), distance(actual, expected) < 0.00001);
}
"#;
