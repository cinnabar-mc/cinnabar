//! Glass plates retain separate body and edge artwork in the carried model.

use super::*;

/// Builds a centered glass plate only from an admitted pane template.
pub(super) fn model(
    world: &RuntimeAssets,
    visual: u32,
) -> Option<([u32; 6], Vec<crate::ActorRigVertex>)> {
    let block = world.resolve(NetworkIdMode::Sequential, visual);
    if block.kind() != VisualKind::Model
        || block.support() != VisualSupport::Exact
        || world
            .model_templates()
            .get(block.model_template()? as usize)?
            .flags
            & assets::MODEL_TEMPLATE_FLAG_PANE
            == 0
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

/// Carries full body UVs and the first two design pixels of each edge texture.
fn quads(materials: [u32; 6]) -> Vec<assets::gui_item::GuiBlockQuad> {
    let half_height = 0.60009766;
    let dimensions = [half_height / 4.0, half_height * 2.0, half_height * 2.0];
    BlockFace::ALL
        .into_iter()
        .map(|face| {
            let (unit, _) = assets::gui_item::cube_face(face);
            let uvs = unit.map(|[x, y, z]| match face {
                BlockFace::West => [z, 1.0 - y],
                BlockFace::East => [1.0 - z, 1.0 - y],
                BlockFace::Down => [x / 8.0, 1.0 - z],
                BlockFace::Up => [x / 8.0, z],
                BlockFace::North => [(1.0 - x) / 8.0, 1.0 - y],
                BlockFace::South => [x / 8.0, 1.0 - y],
            });
            assets::gui_item::GuiBlockQuad {
                corners: unit.map(|point| {
                    std::array::from_fn(|axis| (point[axis] - 0.5) * dimensions[axis] + 0.5)
                }),
                uvs,
                material: materials[match face {
                    BlockFace::West | BlockFace::East => face,
                    _ => BlockFace::Up,
                } as usize],
                two_sided: false,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plate_edges_sample_two_pixels_without_stretching_the_body() {
        let faces = BlockFace::ALL.map(|face| face as u32 + 1);
        let quads = quads(faces);
        assert_eq!(quads.len(), 6);
        for (face, quad) in BlockFace::ALL.into_iter().zip(quads) {
            let body = matches!(face, BlockFace::West | BlockFace::East);
            let max_u = quad.uvs.iter().map(|uv| uv[0]).fold(0.0, f32::max);
            assert_eq!(max_u, if body { 1.0 } else { 0.125 });
            assert_eq!(
                quad.material,
                faces[if body { face } else { BlockFace::Up } as usize]
            );
            assert!(quad.uvs.iter().all(|uv| (0.0..=1.0).contains(&uv[1])));
        }
    }
}
