//! Carried buttons retain their unpressed panel and cropped block-face artwork.

use super::*;

/// Builds an unpressed carried panel independently of the inventory wall mounting.
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

/// Samples the six block-coordinate face projections without stretching the narrow edges.
fn quads(materials: [u32; 6]) -> Vec<assets::gui_item::GuiBlockQuad> {
    let bounds = [[0.3125, 0.6875], [0.875, 1.0], [0.375, 0.625]];
    BlockFace::ALL
        .into_iter()
        .map(|face| {
            let (unit, _) = assets::gui_item::cube_face(face);
            let corners = unit.map(|point| {
                std::array::from_fn(|axis| {
                    bounds[axis][0] + point[axis] * (bounds[axis][1] - bounds[axis][0])
                })
            });
            assets::gui_item::GuiBlockQuad {
                corners,
                uvs: corners.map(|[x, y, z]| match face {
                    BlockFace::Down => [x, 1.0 - z],
                    BlockFace::Up => [x, z],
                    BlockFace::North => [1.0 - x, 1.0 - y],
                    BlockFace::South => [x, 1.0 - y],
                    BlockFace::West => [z, 1.0 - y],
                    BlockFace::East => [1.0 - z, 1.0 - y],
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
    fn carried_buttons_keep_cap_and_edge_texture_landmarks() {
        let quads = quads(BlockFace::ALL.map(|face| face as u32 + 1));
        for (face, point, expected_uv) in [
            (BlockFace::Down, [0.3125, 0.875, 0.625], [0.3125, 0.375]),
            (BlockFace::Up, [0.3125, 1.0, 0.375], [0.3125, 0.375]),
            (BlockFace::North, [0.3125, 1.0, 0.375], [0.6875, 0.0]),
            (BlockFace::South, [0.3125, 1.0, 0.625], [0.3125, 0.0]),
            (BlockFace::West, [0.3125, 1.0, 0.625], [0.625, 0.0]),
            (BlockFace::East, [0.6875, 1.0, 0.625], [0.375, 0.0]),
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
