use super::*;

const ART: Art = Art {
    top: 1,
    side: 2,
    bottom: 3,
};

/// Reads the quad of `quads` lying flat on `face` at the given plane position.
fn on_plane(quads: &[ModelQuad], axis: usize, position: i16) -> Vec<ModelQuad> {
    quads
        .iter()
        .filter(|quad| quad.positions.iter().all(|p| p[axis] == position))
        .copied()
        .collect()
}

/// Reads the smallest and largest UV of a quad on one texture axis.
fn uv_span(quad: &ModelQuad, axis: usize) -> [u16; 2] {
    let values = quad.uvs.map(|uv| uv[axis]);
    [*values.iter().min().unwrap(), *values.iter().max().unwrap()]
}

#[test]
fn the_chain_keeps_each_part_within_one_visibility_mask() {
    let parts = parts(ART);
    assert_eq!(parts.each_ref().map(Vec::len), [16, 24, 22]);
    assert!(
        parts
            .iter()
            .all(|part| part.len() <= assets::MAX_MODEL_TEMPLATE_QUADS)
    );
}

#[test]
fn only_the_foot_part_reaches_down_into_the_lower_ring() {
    let [foot, poles, crown] = parts(ART);
    let ring_top = |quads: &[ModelQuad]| {
        quads
            .iter()
            .flat_map(|quad| quad.positions)
            .map(|p| p[1])
            .max()
            .unwrap()
    };
    assert_eq!(ring_top(&foot), INSET);
    assert_eq!(ring_top(&poles), FULL);
    assert!(
        crown
            .iter()
            .flat_map(|quad| quad.positions)
            .all(|p| p[1] >= FAR)
    );
}

#[test]
fn every_rail_side_reads_a_band_of_the_side_art_inside_its_pole_columns() {
    let [foot, _, crown] = parts(ART);
    for (part, band) in [(&foot, [BAND_END, WHOLE]), (&crown, [0, BAND])] {
        let sides: Vec<_> = part
            .iter()
            .filter(|quad| quad.material == ART.side)
            // The plate's own sides are one model unit tall.
            .filter(|quad| {
                let ys = quad.positions.map(|p| p[1]);
                ys.iter().max().unwrap() - ys.iter().min().unwrap() == INSET
            })
            .collect();
        assert_eq!(sides.len(), 8);
        for quad in sides {
            assert_eq!(uv_span(quad, 0), [BAND, BAND_END]);
            assert_eq!(uv_span(quad, 1), band);
        }
    }
}

#[test]
fn rails_run_along_their_art_in_the_rotated_bottom_texture() {
    let [_, _, crown] = parts(ART);
    let undersides = on_plane(&crown, 1, FAR);
    assert_eq!(undersides.len(), 4);
    for quad in undersides {
        assert_eq!(quad.material, ART.bottom);
        assert_eq!(uv_span(&quad, 0), [BAND, BAND_END]);
        assert_eq!(uv_span(&quad, 1), [BAND_END, WHOLE]);
        // The long edge of the rail must vary only u, in whichever axis it runs.
        let [first, second, ..] = quad.positions;
        let along_x = (first[0] - second[0]).abs() > INSET;
        let along_z = (first[2] - second[2]).abs() > INSET;
        let [a, b, ..] = quad.uvs;
        if along_x || along_z {
            assert_eq!(a[1], b[1], "the long edge stays in one row: {:?}", quad.uvs);
        } else {
            assert_eq!(
                a[0], b[0],
                "the short edge stays in one column: {:?}",
                quad.uvs
            );
        }
    }
}

#[test]
fn corner_poles_show_the_pole_columns_of_the_side_art() {
    let [_, poles, _] = parts(ART);
    let sides: Vec<_> = poles
        .iter()
        .filter(|quad| quad.material == ART.side)
        .collect();
    assert_eq!(sides.len(), 16);
    for quad in sides {
        assert!(matches!(uv_span(quad, 0), [0, BAND] | [BAND_END, WHOLE]));
        assert_eq!(uv_span(quad, 1), [0, WHOLE]);
    }
}

#[test]
fn up_and_down_rotations_match_the_native_corner_tables() {
    let (low, high) = ([0, 0, 0], [FULL, FULL, FULL]);
    let rect = [10, 20, 30, 40];
    let corner = |face, flip, x: bool, z: bool| {
        let quad = textured(1, face, low, high, rect, flip);
        let index = quad
            .positions
            .iter()
            .position(|p| (p[0] == FULL) == x && (p[2] == FULL) == z)
            .unwrap();
        quad.uvs[index]
    };
    assert_eq!(corner(BlockFace::Up, 0, false, false), [10, 30]);
    assert_eq!(corner(BlockFace::Up, 1, false, false), [10, 40]);
    assert_eq!(corner(BlockFace::Up, 2, false, false), [20, 30]);
    assert_eq!(corner(BlockFace::Up, 3, false, false), [20, 40]);
    assert_eq!(corner(BlockFace::Down, 0, false, false), [10, 40]);
    assert_eq!(corner(BlockFace::Down, 1, false, false), [20, 40]);
    assert_eq!(corner(BlockFace::Down, 2, false, false), [10, 30]);
    assert_eq!(corner(BlockFace::Down, 3, false, false), [20, 30]);
}

#[test]
fn a_pole_foot_reads_its_position_in_the_bottom_art_mirrored_within_itself() {
    let [_, poles, _] = parts(ART);
    let foot = poles
        .iter()
        .find(|quad| {
            quad.material == ART.bottom
                && quad
                    .positions
                    .iter()
                    .all(|p| p[0] <= INSET && p[2] <= INSET)
        })
        .expect("pole at the low corner");
    for (position, uv) in foot.positions.iter().zip(foot.uvs) {
        let expected_v = if position[2] == 0 { BAND } else { 0 };
        assert_eq!(uv[1], expected_v, "{position:?}");
    }
}

#[test]
fn quads_are_wound_to_face_outward() {
    let [foot, poles, crown] = parts(ART);
    for quad in foot.iter().chain(&poles).chain(&crown) {
        let [p0, p1, p2, _] = quad.positions.map(|p| p.map(i32::from));
        let a = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let b = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
        let normal = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        let expected = match quad.flags & MODEL_QUAD_FLAG_FACE_MASK {
            1 => [0, -1, 0],
            2 => [0, 1, 0],
            3 => [-1, 0, 0],
            4 => [1, 0, 0],
            5 => [0, 0, -1],
            _ => [0, 0, 1],
        };
        assert_eq!(normal.map(i32::signum), expected, "{:?}", quad.positions);
    }
}
