use serde_json::{Value, json};

use super::{Cube, FaceQuad, Geometry, parse_cube};

const WEST: f32 = 0.0;
const EAST: f32 = 16.0;
const NORTH: f32 = 0.0;
const SOUTH: f32 = 16.0;

fn full_cube(uv: Value) -> Cube {
    parse_cube(&json!({ "origin": [-8, 0, -8], "size": [16, 16, 16], "uv": uv })).unwrap()
}

fn face(cube: &Cube, index: usize) -> FaceQuad {
    cube.quads()
        .into_iter()
        .map(|(quad, _)| quad)
        .find(|quad| quad.face == index)
        .unwrap()
}

/// The UV a face assigns to one of its world-space X/Z corners.
fn uv_at(quad: &FaceQuad, x: f32, z: f32) -> [f32; 2] {
    let corner = quad
        .positions
        .iter()
        .position(|position| position[0] == x && position[2] == z)
        .unwrap();
    quad.uvs[corner]
}

fn corners(quad: &FaceQuad) -> [[f32; 2]; 4] {
    [
        uv_at(quad, EAST, SOUTH),
        uv_at(quad, EAST, NORTH),
        uv_at(quad, WEST, NORTH),
        uv_at(quad, WEST, SOUTH),
    ]
}

#[test]
fn per_face_top_starts_south_east_and_bottom_starts_north_east() {
    let cube = full_cube(json!({
        "up": { "uv": [4, 8], "uv_size": [2, 3] },
        "down": { "uv": [4, 8], "uv_size": [2, 3] },
    }));
    assert_eq!(
        corners(&face(&cube, 3)),
        [[4.0, 8.0], [4.0, 11.0], [6.0, 11.0], [6.0, 8.0]],
    );
    assert_eq!(
        corners(&face(&cube, 2)),
        [[4.0, 11.0], [4.0, 8.0], [6.0, 8.0], [6.0, 11.0]],
    );
}

#[test]
fn half_turned_per_face_top_starts_at_the_opposite_corner() {
    let cube = full_cube(json!({
        "up": { "uv": [4, 8], "uv_size": [2, 3], "uv_rotation": 180 },
    }));
    assert_eq!(
        corners(&face(&cube, 3)),
        [[6.0, 11.0], [6.0, 8.0], [4.0, 8.0], [4.0, 11.0]],
    );
}

#[test]
fn box_layout_maps_top_and_bottom_from_the_cross() {
    let cube = parse_cube(&json!({
        "origin": [-8, 0, -8], "size": [16, 2, 16], "uv": [10, 20],
    }))
    .unwrap();
    assert_eq!(
        corners(&face(&cube, 3)),
        [[26.0, 20.0], [26.0, 36.0], [42.0, 36.0], [42.0, 20.0]],
    );
    assert_eq!(
        corners(&face(&cube, 2)),
        [[42.0, 20.0], [42.0, 36.0], [58.0, 36.0], [58.0, 20.0]],
    );
}

#[test]
fn only_per_face_cubes_report_a_legacy_top_mapping() {
    assert!(full_cube(json!({ "up": { "uv": [0, 0], "uv_size": [1, 1] } })).per_face_uv);
    assert!(!full_cube(json!([0, 0])).per_face_uv);
    assert!(!Geometry::full_block().cubes[0].per_face_uv);
}

#[test]
fn displaced_full_block_samples_like_the_cube_path() {
    let block = Geometry::full_block();
    let cube = &block.cubes[0];
    assert_eq!(
        corners(&face(cube, 3)),
        [[16.0, 16.0], [16.0, 0.0], [0.0, 0.0], [0.0, 16.0]],
    );
    assert_eq!(
        corners(&face(cube, 2)),
        [[16.0, 0.0], [16.0, 16.0], [0.0, 16.0], [0.0, 0.0]],
    );
}

/// The world-space X/Z corners a built-in cross plane reaches, deduplicated and ordered by X.
fn cross_plane(cube: &Cube) -> Vec<[f32; 2]> {
    let mut points = cube
        .quads()
        .into_iter()
        .flat_map(|(quad, _)| quad.positions)
        .map(|position| [position[0], position[2]])
        .collect::<Vec<_>>();
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4);
    points
}

#[test]
fn builtin_cross_is_two_diagonal_planes_using_the_whole_texture() {
    let cross = Geometry::cross();
    assert_eq!(cross.texture_size, [16.0; 2]);
    assert_eq!((cross.cubes.len(), cross.skipped_cubes), (2, 0));
    let inset = 8.0 - 10.2 * std::f32::consts::FRAC_1_SQRT_2;
    let far = 16.0 - inset;
    for (cube, [near, opposite]) in cross
        .cubes
        .iter()
        .zip([[[inset, far], [far, inset]], [[inset, inset], [far, far]]])
    {
        assert_eq!((cube.bone.as_str(), cube.per_face_uv), ("cube", true));
        let [(pivot, angles)] = cube.rotations[..] else {
            panic!("cross planes turn once each");
        };
        assert_eq!(pivot, [8.0; 3]);
        assert_eq!((angles[0], angles[1].abs(), angles[2]), (0.0, 45.0, 0.0));
        let points = cross_plane(cube);
        assert_eq!(
            points.len(),
            2,
            "one plane reaches two distinct X/Z corners"
        );
        for (point, expected) in points.iter().zip([near, opposite]) {
            assert!(
                (point[0] - expected[0]).abs() < 1e-3 && (point[1] - expected[1]).abs() < 1e-3,
                "plane corner {point:?} is not {expected:?}"
            );
        }
        let quads = cube.quads();
        assert_eq!(quads.len(), 2, "north and south sides of one plane");
        for (quad, instance) in quads {
            assert!(matches!(quad.face, 4 | 5));
            assert_eq!(instance, Some("cross"));
            let y = quad.positions.map(|position| position[1]);
            assert_eq!(
                (
                    y.iter().fold(f32::INFINITY, |a, &b| a.min(b)),
                    y.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b))
                ),
                (0.0, 16.0)
            );
            let mut uvs = quad.uvs.to_vec();
            uvs.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
            assert_eq!(uvs, [[0.0, 0.0], [0.0, 16.0], [16.0, 0.0], [16.0, 16.0]]);
        }
    }
}
