//! Bamboo inventory posts and slats with independent texture regions and paint order.

use super::{BlockFace, CUBE_FACES, GuiBlockQuad, INVENTORY_FENCE_BOXES, cube_face, fence_face};

/// Width of a post's texture region, ending just inside its fourth source texel.
const POST_TEXTURE_WIDTH: f32 = 255.0 / 64.0;

/// Returns the visible inventory faces in their projected paint order.
pub(super) fn quads(material: u32) -> Vec<GuiBlockQuad> {
    build(material, false)
}

/// Retains every carried surface of the separate bamboo posts and thicker slats.
#[must_use]
pub fn carried_quads(material: u32) -> Vec<GuiBlockQuad> {
    build(material, true)
}

/// Shares bamboo geometry and texture regions across its GUI and carried presentations.
fn build(material: u32, carried: bool) -> Vec<GuiBlockQuad> {
    let mut out = Vec::with_capacity(if carried { 48 } else { 24 });
    let posts = [INVENTORY_FENCE_BOXES[0], INVENTORY_FENCE_BOXES[1]];
    for (index, (span, top_v, side_u)) in [
        ([-2, 0], [7.0, 9.0], [8.0, 10.0]),
        ([4, 12], [7.0, 15.0], [8.0, 16.0]),
        ([16, 18], [13.0, 15.0], [14.0, 16.0]),
    ]
    .into_iter()
    .enumerate()
    {
        for [bottom, top] in [[12, 15], [4, 7]] {
            append(
                &mut out,
                [7, bottom, span[0]],
                [9, top, span[1]],
                [
                    [11.0, top_v[0], 13.0, top_v[1]],
                    [13.0, 4.0, 15.0, 7.0],
                    [side_u[0], 0.0, side_u[1], 3.0],
                ],
                material,
                carried,
            );
        }
        if let Some(&(min, max)) = posts.get(index) {
            append(
                &mut out,
                min,
                max,
                [
                    [4.0, 0.0, 4.0 + POST_TEXTURE_WIDTH, 4.0],
                    [0.0, 0.0, POST_TEXTURE_WIDTH, 16.0],
                    [0.0, 0.0, POST_TEXTURE_WIDTH, 16.0],
                ],
                material,
                carried,
            );
        }
    }
    out
}

/// Appends projected surfaces and, when carried, their outward-facing counterparts.
fn append(
    out: &mut Vec<GuiBlockQuad>,
    min: [i8; 3],
    max: [i8; 3],
    regions: [[f32; 4]; 3],
    material: u32,
    carried: bool,
) {
    for ((face, unit, unit_uv, _), [u0, v0, u1, v1]) in CUBE_FACES.into_iter().zip(regions) {
        let corners = unit.map(|point| {
            std::array::from_fn(|axis| {
                (f32::from(min[axis]) + point[axis] * f32::from(max[axis] - min[axis])) / 16.0
            })
        });
        let uvs = unit_uv.map(|[u, v]| [(u0 + u * (u1 - u0)) / 16.0, (v0 + v * (v1 - v0)) / 16.0]);
        out.push(fence_face(face, corners, uvs, material));
    }
    if carried {
        for (face, [u0, v0, u1, v1]) in [BlockFace::Down, BlockFace::North, BlockFace::East]
            .into_iter()
            .zip(regions)
        {
            let (unit, _) = cube_face(face);
            let corners = unit.map(|point| {
                std::array::from_fn(|axis| {
                    (f32::from(min[axis]) + point[axis] * f32::from(max[axis] - min[axis])) / 16.0
                })
            });
            let uvs = unit.map(|[x, y, z]| {
                let [u, v] = match face {
                    BlockFace::Down => [x, z],
                    BlockFace::North => [1.0 - x, 1.0 - y],
                    BlockFace::East if min[2] < 0 || max[2] > 16 => [z, 1.0 - y],
                    BlockFace::East => [1.0 - z, 1.0 - y],
                    _ => unreachable!("only the three carried counterparts are appended"),
                };
                [(u0 + u * (u1 - u0)) / 16.0, (v0 + v * (v1 - v0)) / 16.0]
            });
            out.push(fence_face(face, corners, uvs, material));
        }
    }
}
