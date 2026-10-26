use super::{carried_fences::uv_at, *};

#[test]
fn carried_walls_keep_post_artwork_and_mirrored_rail_edges() {
    let quads = carried_wall_quads(7);
    assert_eq!(quads.len(), 24);
    assert!(
        quads
            .iter()
            .all(|quad| quad.material == 7 && !quad.two_sided)
    );
    for (face, point, uv) in [
        (BlockFace::Up, [4., 16., 12.], [4., 0.]),
        (BlockFace::Down, [4., 0., 12.], [4., 8.]),
        (BlockFace::North, [4., 16., 12.], [12., 0.]),
        (BlockFace::South, [4., 16., 20.], [4., 0.]),
        (BlockFace::West, [4., 0., 12.], [0., 16.]),
        (BlockFace::East, [12., 0., 12.], [8., 16.]),
        (BlockFace::Up, [4., 16., -4.], [4., 0.]),
        (BlockFace::Down, [4., 0., -4.], [4., 8.]),
        (BlockFace::East, [12., 16., 4.], [0., 0.]),
        (BlockFace::Up, [5., 13., 12.], [5., 12.]),
        (BlockFace::Down, [5., 0., 12.], [5., 8.]),
        (BlockFace::North, [5., 13., 4.], [11., 3.]),
        (BlockFace::South, [5., 13., 12.], [5., 3.]),
        (BlockFace::West, [5., 13., 12.], [12., 3.]),
        (BlockFace::East, [11., 13., 12.], [8., 3.]),
        (BlockFace::Up, [5., 13., 4.], [5., 4.]),
        (BlockFace::Down, [5., 0., 4.], [5., 8.]),
        (BlockFace::West, [5., 13., 4.], [4., 3.]),
        (BlockFace::East, [11., 0., 4.], [8., 16.]),
    ] {
        assert_eq!(
            uv_at(&quads, face, point),
            Some(uv.map(|v| v / 16.0)),
            "{face:?} at {point:?}"
        );
    }
}

#[test]
fn carried_walls_split_the_span_at_the_center_between_posts() {
    let quads = carried_wall_quads(7);
    let mut bounds: Vec<_> = quads
        .chunks_exact(6)
        .map(|part| {
            let corners: Vec<_> = part.iter().flat_map(|quad| quad.corners).collect();
            std::array::from_fn::<_, 3, _>(|axis| {
                [
                    corners
                        .iter()
                        .map(|p| p[axis])
                        .fold(f32::INFINITY, f32::min),
                    corners
                        .iter()
                        .map(|p| p[axis])
                        .fold(f32::NEG_INFINITY, f32::max),
                ]
            })
        })
        .collect();
    bounds.sort_by(|a, b| a[2][0].total_cmp(&b[2][0]));
    assert_eq!(
        bounds,
        [
            [[0.25, 0.75], [0., 1.], [-0.25, 0.25]],
            [[0.3125, 0.6875], [0., 0.8125], [0.25, 0.5]],
            [[0.3125, 0.6875], [0., 0.8125], [0.5, 0.75]],
            [[0.25, 0.75], [0., 1.], [0.75, 1.25]],
        ]
    );
}
