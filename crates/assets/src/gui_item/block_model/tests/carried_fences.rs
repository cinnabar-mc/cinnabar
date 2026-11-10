use super::*;

#[test]
fn carried_fences_keep_post_caps_and_rail_texture_landmarks() {
    let quads = carried_fence_quads(7);
    assert_eq!(quads.len(), 32);
    for quad in &quads {
        assert_eq!(quad.material, 7);
        assert!(!quad.two_sided);
        assert!(quad.uvs.iter().flatten().all(|v| (0.0..=1.0).contains(v)));
    }
    for (face, point, expected_uv) in [
        (BlockFace::Up, [6., 16., 0.], [6., 6.]),
        (BlockFace::Up, [6., 16., 12.], [6., 6.]),
        (BlockFace::Down, [6., 0., 0.], [6., 10.]),
        (BlockFace::North, [10., 16., 0.], [6., 0.]),
        (BlockFace::East, [10., 16., 0.], [10., 0.]),
        (BlockFace::Up, [7., 15., -2.], [7., 4.]),
        (BlockFace::Up, [7., 15., 18.], [7., 12.]),
        (BlockFace::Down, [7., 13., -2.], [7., 14.]),
        (BlockFace::Down, [7., 13., 18.], [7., 2.]),
        (BlockFace::West, [7., 15., -2.], [4., 0.]),
        (BlockFace::East, [9., 15., -2.], [4., 2.]),
        (BlockFace::West, [7., 7., -2.], [4., 7.]),
        (BlockFace::East, [9., 7., -2.], [4., 9.]),
        (BlockFace::East, [9., 5., 18.], [12., 7.]),
    ] {
        let point = point.map(|v| v / 16.0);
        let (axis, positive) = match face {
            BlockFace::West => (0, false),
            BlockFace::East => (0, true),
            BlockFace::Down => (1, false),
            BlockFace::Up => (1, true),
            BlockFace::North => (2, false),
            BlockFace::South => (2, true),
        };
        let uv = quads.iter().find_map(|quad| {
            let [a, b, c, _] = quad.corners;
            let u: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
            let v: [f32; 3] = std::array::from_fn(|i| c[i] - a[i]);
            let i = (axis + 1) % 3;
            let j = (axis + 2) % 3;
            let normal = u[i] * v[j] - u[j] * v[i];
            if normal == 0.0 || normal.is_sign_positive() != positive {
                return None;
            }
            quad.corners
                .iter()
                .position(|corner| *corner == point)
                .map(|corner| quad.uvs[corner])
        });
        assert_eq!(
            uv,
            Some(expected_uv.map(|v| v / 16.0)),
            "{face:?} at {point:?}"
        );
    }
}
