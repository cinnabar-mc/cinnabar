use super::*;

// A cube turned about Y turns its top and bottom art with it (a cardinal_direction tile's arrow
// points where the block faces), as the model path's rotated quads already do.
#[test]
fn turned_cubes_turn_their_top_and_bottom_art() {
    let blocks = CustomBlocks {
        blocks: (0..4)
            .map(|quarters| {
                block(
                    &format!("test:turned_{quarters}"),
                    1,
                    CustomBlockVisuals {
                        base: CustomVisualComponents {
                            materials: materials("lucky"),
                            ..turn(quarters as i32)
                        },
                        ..CustomBlockVisuals::default()
                    },
                )
            })
            .collect::<Vec<_>>()
            .into(),
        ..Default::default()
    };
    let compiled = compile_block_overlay(&view(), &blocks, false, None).unwrap();
    let code = |visual: usize, face: BlockFace| {
        let visual = compiled.overlay.visuals[visual];
        assert_eq!(visual.kind, VisualKind::Cube, "a quarter turn keeps the cube path");
        compiled.overlay.materials[visual.faces[face as usize] as usize].flags
            & assets::MATERIAL_FLAG_UV_MASK
    };
    // Seen from above a positive quarter turn moves north to west: the top art turns the same
    // way (packed code 3 is the opposite of code 1); seen from below it turns the other way.
    for (quarters, up, down) in [(0_usize, 0, 0), (1, 3, 1), (2, 2, 2), (3, 1, 3)] {
        assert_eq!(code(quarters, BlockFace::Up), up, "top after {quarters} quarter turns");
        assert_eq!(code(quarters, BlockFace::Down), down, "bottom after {quarters} quarter turns");
        for side in [BlockFace::West, BlockFace::East, BlockFace::North, BlockFace::South] {
            assert_eq!(code(quarters, side), 0, "side art stays upright about Y");
        }
    }
}

// Every quarter-turn combination maps each model face's texture axes onto the world face it
// lands on, so the art never shears or mirrors on any axis.
#[test]
fn turned_face_uv_flags_follow_every_quarter_turn() {
    for x in 0..4 {
        for y in 0..4 {
            for z in 0..4 {
                let rotation = [x, y, z];
                for source in 0..6 {
                    let face = super::super::rotate_face(source, rotation);
                    let flags = super::super::turned_face_uv_flags(source, face, rotation);
                    assert_eq!(
                        flags & !assets::MATERIAL_FLAG_UV_ROTATION_MASK,
                        0,
                        "a rotation never needs a reflection ({rotation:?}, face {source})"
                    );
                }
            }
        }
    }
}
