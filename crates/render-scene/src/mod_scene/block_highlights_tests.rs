#[cfg(test)]
mod tests {
    use crate::ModRenderScene;
    use mod_api::MAX_BLOCK_HIGHLIGHTS;
    use mod_render::block_highlights::*;
    use mod_render::geometry::{KIND_GROUND, STYLE_BILLBOARD};
    use std::sync::Arc;

    const PINK: [f32; 4] = [1.0, 0.08, 0.45, 1.0];

    #[test]
    fn cubes_cover_six_entire_faces_without_edge_geometry() {
        let position = [-17, 23, 91];
        let vertices = build(&[position], PINK);
        assert_eq!(vertices.len(), VERTICES_PER_BLOCK);
        for face in vertices.as_chunks::<6>().0 {
            let points: Vec<_> = face
                .iter()
                .map(|v| Vec3::from_array(v.anchor[..3].try_into().unwrap()))
                .collect();
            let normal = (points[1] - points[0]).cross(points[2] - points[0]);
            assert_eq!(normal.length(), 1.0, "each triangle fills half a unit face");
            let second = (points[4] - points[3]).cross(points[5] - points[3]);
            assert_eq!(normal, second);
            assert_eq!(
                (0..3)
                    .filter(|axis| face
                        .iter()
                        .all(|v| v.anchor[*axis] == face[0].anchor[*axis]))
                    .count(),
                1
            );
        }
        assert!(vertices.iter().all(|v| v.anchor[3] == KIND_GROUND
            && v.color == PINK
            && v.style[0] == STYLE_BILLBOARD));
        for (axis, &low) in position.iter().enumerate() {
            assert_eq!(
                vertices
                    .iter()
                    .map(|v| v.anchor[axis])
                    .fold(f32::INFINITY, f32::min),
                low as f32
            );
            assert_eq!(
                vertices
                    .iter()
                    .map(|v| v.anchor[axis])
                    .fold(f32::NEG_INFINITY, f32::max),
                low as f32 + 1.0
            );
        }
    }

    use bevy::prelude::Vec3;

    fn block(position: [i32; 3]) -> mod_api::native_render::BlockHighlight {
        mod_api::native_render::BlockHighlight {
            position,
            colour: PINK,
            outline: None,
        }
    }

    #[test]
    fn unchanged_blocks_reuse_geometry_and_preserve_guest_output() {
        let mut scene = ModRenderScene::default();
        let guest = Arc::clone(&scene.vertices);
        scene.set_drawn_block_highlights(&[block([2, 3, 4])]);
        let vertices = Arc::clone(&scene.block_vertices);
        scene.set_drawn_block_highlights(&[block([2, 3, 4])]);
        assert!(Arc::ptr_eq(&vertices, &scene.block_vertices));
        assert!(Arc::ptr_eq(&guest, &scene.vertices));
        scene.set_drawn_block_highlights(&[]);
        assert!(scene.block_vertices.is_empty());
    }

    #[test]
    fn publication_caps_blocks() {
        let mut scene = ModRenderScene::default();
        let separate: Vec<_> = (0..=MAX_BLOCK_HIGHLIGHTS as i32)
            .map(|index| block([2 * index, 0, 0]))
            .collect();
        scene.set_drawn_block_highlights(&separate);
        assert_eq!(
            scene.block_vertices.len(),
            MAX_BLOCK_HIGHLIGHTS * VERTICES_PER_BLOCK
        );
        scene.clear();
        assert_eq!(scene.vertex_count(), 0);
    }
}

/// Packet-provided colours use exactly the cube geometry and release together.
#[test]
fn drawn_blocks_use_cube_geometry_and_release() {
    use super::*;
    let mut scene = ModRenderScene::default();
    let red = [1., 0., 0., 1.];
    scene.set_drawn_block_highlights(&[mod_api::native_render::BlockHighlight {
        position: [4, 5, 6],
        colour: red,
        outline: None,
    }]);
    assert_eq!(
        scene.block_vertices.as_ref(),
        mod_render::block_highlights::build(&[[4, 5, 6]], red)
    );
    scene.set_drawn_block_highlights(&[]);
    assert!(scene.block_vertices.is_empty());
}
