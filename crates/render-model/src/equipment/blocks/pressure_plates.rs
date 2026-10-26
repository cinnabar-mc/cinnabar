//! Carried pressure plates keep a thin panel and their own horizontal pivot.

use super::*;

const EDGE: f32 = 1.0 / 16.0;
const SPAN: f32 = 1.0 - 2.0 * EDGE;

/// Builds the carried panel independently of its floor-mounted placement state.
pub(super) fn model(
    world: &RuntimeAssets,
    visual: u32,
) -> Option<([u32; 6], Vec<crate::ActorRigVertex>)> {
    let block = world.resolve(NetworkIdMode::Sequential, visual);
    if block.kind() != VisualKind::Model
        || block.support() != VisualSupport::Exact
        || block.model_template().is_none()
        || block.animation().is_some()
    {
        return None;
    }
    template_quads(&quads(
        BlockFace::ALL.map(|face| block.face(face).material_id()),
    ))
}

/// Preserves inset cap artwork and the bottom source-texture row on narrow edges.
fn quads(materials: [u32; 6]) -> Vec<assets::gui_item::GuiBlockQuad> {
    BlockFace::ALL
        .into_iter()
        .map(|face| {
            let (unit, _) = assets::gui_item::cube_face(face);
            assets::gui_item::GuiBlockQuad {
                corners: unit.map(|[x, y, z]| {
                    [
                        2.0 * EDGE + x * SPAN,
                        0.5 + (y - 0.5) * EDGE,
                        2.0 * EDGE + z * SPAN,
                    ]
                }),
                uvs: unit.map(|[x, y, z]| {
                    let u = EDGE + x * SPAN;
                    let v = EDGE + z * SPAN;
                    let side = 1.0 - y * EDGE;
                    match face {
                        BlockFace::Down => [u, 1.0 - v],
                        BlockFace::Up => [u, v],
                        BlockFace::North => [1.0 - u, side],
                        BlockFace::South => [u, side],
                        BlockFace::West => [v, side],
                        BlockFace::East => [1.0 - v, side],
                    }
                }),
                material: materials[face as usize],
                two_sided: false,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carried_pressure_plates_keep_inset_caps_and_narrow_edge_artwork() {
        let quads = quads(BlockFace::ALL.map(|face| face as u32 + 1));
        for (face, point, expected_uv) in [
            (BlockFace::Down, [0.125, 0.46875, 1.0], [0.0625, 0.0625]),
            (BlockFace::Up, [0.125, 0.53125, 0.125], [0.0625, 0.0625]),
            (BlockFace::North, [0.125, 0.53125, 0.125], [0.9375, 0.9375]),
            (BlockFace::South, [0.125, 0.53125, 1.0], [0.0625, 0.9375]),
            (BlockFace::West, [0.125, 0.53125, 0.125], [0.0625, 0.9375]),
            (BlockFace::East, [1.0, 0.53125, 0.125], [0.9375, 0.9375]),
            (BlockFace::South, [0.125, 0.46875, 1.0], [0.0625, 1.0]),
        ] {
            let quad = &quads[face as usize];
            let corner = quad
                .corners
                .iter()
                .position(|corner| *corner == point)
                .unwrap();
            assert_eq!(quad.material, face as u32 + 1);
            assert_eq!(quad.uvs[corner], expected_uv, "{face:?}");
        }
    }
}
