//! Carried trapdoors use a centered panel independently of their placement state.

use super::*;

const THICKNESS: f32 = 0.1825;

/// Builds a carried panel from the six faces of an admitted trapdoor model.
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
    let (materials, mut vertices) = template_quads(&quads(
        BlockFace::ALL.map(|face| block.face(face).material_id()),
    ))?;
    for vertex in &mut vertices {
        vertex.back_uv = crate::ONE_SIDED_BACK_UV;
    }
    Some((materials, vertices))
}

/// Preserves cap orientation and samples only the thin edge strip on side faces.
fn quads(materials: [u32; 6]) -> Vec<assets::gui_item::GuiBlockQuad> {
    let half_height = half::f16::from_f32(THICKNESS * 0.5).to_f32();
    BlockFace::ALL
        .into_iter()
        .map(|face| {
            let (unit, _) = assets::gui_item::cube_face(face);
            assets::gui_item::GuiBlockQuad {
                corners: unit.map(|[x, y, z]| [x, 0.5 + (y * 2.0 - 1.0) * half_height, z]),
                uvs: unit.map(|[x, y, z]| match face {
                    BlockFace::Up => [x, z],
                    BlockFace::Down => [x, 1.0 - z],
                    BlockFace::North => [x, y * THICKNESS],
                    BlockFace::South => [1.0 - x, y * THICKNESS],
                    BlockFace::West => [1.0 - z, y * THICKNESS],
                    BlockFace::East => [z, y * THICKNESS],
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
    fn carried_panel_caps_and_edges_keep_their_authored_orientation() {
        let quads = quads(BlockFace::ALL.map(|face| face as u32 + 1));
        for (face, quad) in BlockFace::ALL.into_iter().zip(quads) {
            assert_eq!(quad.material, face as u32 + 1);
            for (point, uv) in quad.corners.into_iter().zip(quad.uvs) {
                let [x, y, z] = point;
                let edge_v = if y > 0.5 { 0.1825 } else { 0.0 };
                let expected = match face {
                    BlockFace::Up => [x, z],
                    BlockFace::Down => [x, 1.0 - z],
                    BlockFace::North => [x, edge_v],
                    BlockFace::South => [1.0 - x, edge_v],
                    BlockFace::West => [1.0 - z, edge_v],
                    BlockFace::East => [z, edge_v],
                };
                assert_eq!(uv, expected, "{face:?} at {point:?}");
            }
        }
    }
}
