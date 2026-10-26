//! Wall posts and connecting spans for inventory and carried presentations.

use super::{BlockFace, GuiBlockQuad, carried_cuboid_faces, carried_face_uvs, cube_face};

/// Two posts and the inventory span, in design pixels.
const BOXES: [([i16; 3], [i16; 3]); 3] = [
    ([4, 0, 12], [12, 16, 20]),
    ([4, 0, -4], [12, 16, 4]),
    ([5, 0, 1], [11, 13, 15]),
];

/// Retains the inventory model's orientation and authored texture regions.
pub(super) fn inventory_quads(materials: [u32; 6]) -> Vec<GuiBlockQuad> {
    let post_uvs = [
        [0, 0, 8, 16],
        [0, 0, 8, 16],
        [8, 8, 0, 0],
        [8, 8, 0, 0],
        [0, 0, 8, 16],
        [0, 0, 8, 16],
    ];
    let rail_uvs = [
        [0, 0, 4, 13],
        [0, 0, 4, 13],
        [6, 4, 0, 0],
        [6, 4, 0, 0],
        [0, 0, 6, 13],
        [0, 0, 6, 13],
    ];
    let mut out = Vec::with_capacity(BOXES.len() * 6);
    for ((min, max), uvs) in BOXES.into_iter().zip([post_uvs, post_uvs, rail_uvs]) {
        for (face, (corners, uv)) in carried_cuboid_faces(min.map(|v| v * 16), max.map(|v| v * 16))
            .into_iter()
            .zip(uvs)
            .enumerate()
        {
            out.push(GuiBlockQuad {
                corners: corners.map(|[x, y, z]| [z, y, 1.0 - x]),
                uvs: carried_face_uvs(face, uv),
                material: materials[match BlockFace::ALL[face] {
                    BlockFace::West => BlockFace::South,
                    BlockFace::East => BlockFace::North,
                    BlockFace::North => BlockFace::West,
                    BlockFace::South => BlockFace::East,
                    vertical => vertical,
                } as usize],
                two_sided: false,
            });
        }
    }
    out
}

/// Builds both posts and separate rail halves on the carried wall's longitudinal axis.
#[must_use]
pub fn carried_quads(material: u32) -> Vec<GuiBlockQuad> {
    let [
        (far_min, far_max),
        (near_min, near_max),
        (rail_min, rail_max),
    ] = BOXES;
    let midpoint = (near_max[2] + far_min[2]) / 2;
    let mut boxes = vec![(far_min, far_max, true)];
    for (start, end) in [(midpoint, far_min[2]), (near_max[2], midpoint)] {
        let (mut min, mut max) = (rail_min, rail_max);
        min[2] = start;
        max[2] = end;
        boxes.push((min, max, false));
    }
    boxes.push((near_min, near_max, true));
    boxes
        .into_iter()
        .flat_map(|(min, max, post)| {
            BlockFace::ALL.map(|face| {
                let (unit, _) = cube_face(face);
                let corners = unit.map(|point| {
                    std::array::from_fn(|axis| {
                        (f32::from(min[axis]) + point[axis] * f32::from(max[axis] - min[axis]))
                            / 16.0
                    })
                });
                let zmin = f32::from(min[2]) / 16.0;
                let zmax = f32::from(max[2]) / 16.0;
                let uvs = corners.map(|[x, y, z]| match face {
                    BlockFace::Up => [x, if post { z - zmin } else { z }],
                    BlockFace::Down => [x, if post { zmax - z } else { zmin + zmax - z }],
                    BlockFace::North => [1.0 - x, 1.0 - y],
                    BlockFace::South => [x, 1.0 - y],
                    BlockFace::West => [if post { z - zmin } else { z }, 1.0 - y],
                    BlockFace::East => [if post { zmax - z } else { zmin + zmax - z }, 1.0 - y],
                });
                GuiBlockQuad {
                    corners,
                    uvs,
                    material,
                    two_sided: false,
                }
            })
        })
        .collect()
}
