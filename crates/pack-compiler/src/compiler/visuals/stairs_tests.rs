use super::*;

/// The world face and corner a model-shader turn puts a template corner on, computed about the
/// block centre as `rotate_cross` does.
fn world_corner(face: BlockFace, [x, y, z]: [i16; 3], turns: u32) -> (BlockFace, [i16; 3]) {
    let normal = match face {
        BlockFace::West => [-1, 0, 0],
        BlockFace::East => [1, 0, 0],
        BlockFace::Down => [0, -1, 0],
        BlockFace::Up => [0, 1, 0],
        BlockFace::North => [0, 0, -1],
        BlockFace::South => [0, 0, 1],
    };
    let (mut normal, mut centred) = (normal, [x - 128, y, z - 128]);
    for _ in 0..turns {
        normal = [-normal[2], normal[1], normal[0]];
        centred = [-centred[2], centred[1], centred[0]];
    }
    let face = match normal {
        [-1, 0, 0] => BlockFace::West,
        [1, 0, 0] => BlockFace::East,
        [0, -1, 0] => BlockFace::Down,
        [0, 1, 0] => BlockFace::Up,
        [0, 0, -1] => BlockFace::North,
        _ => BlockFace::South,
    };
    (face, [centred[0] + 128, centred[1], centred[2] + 128])
}

/// Every stair face reads the native cube UVs of where it lands in the world, so the same
/// world point samples the same texel whichever way the stair faces.
#[test]
fn stair_faces_use_world_space_cube_uvs_for_every_facing() {
    for turns in 0..4 {
        for upside_down in [false, true] {
            for shape in 0..5 {
                for quad in stair_quads([1; 6], 2, upside_down, shape, turns) {
                    let face = match quad.flags & MODEL_QUAD_FLAG_FACE_MASK {
                        1 => BlockFace::Down,
                        2 => BlockFace::Up,
                        3 => BlockFace::West,
                        4 => BlockFace::East,
                        5 => BlockFace::North,
                        _ => BlockFace::South,
                    };
                    for (position, uv) in quad.positions.into_iter().zip(quad.uvs) {
                        let (world_face, [x, y, z]) = world_corner(face, position, turns);
                        let [x, y, z] = [x, y, z].map(|value| value as u16 * 16);
                        let expected = match world_face {
                            BlockFace::West => [z, 4096 - y],
                            BlockFace::East => [4096 - z, 4096 - y],
                            BlockFace::Down => [x, 4096 - z],
                            BlockFace::Up => [x, z],
                            BlockFace::North => [4096 - x, 4096 - y],
                            BlockFace::South => [x, 4096 - y],
                        };
                        assert_eq!(
                            uv, expected,
                            "turns {turns} upside {upside_down} shape {shape} {face:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn turned_faces_follow_the_model_turn() {
    for turns in 0..4 {
        for face in BlockFace::ALL {
            let centre = match face {
                BlockFace::West => [0, 128, 128],
                BlockFace::East => [256, 128, 128],
                BlockFace::Down => [128, 0, 128],
                BlockFace::Up => [128, 256, 128],
                BlockFace::North => [128, 128, 0],
                BlockFace::South => [128, 128, 256],
            };
            let (expected_face, expected_position) = world_corner(face, centre, turns);
            assert_eq!(turned_face(face, turns), expected_face);
            assert_eq!(turned_position(centre, turns), expected_position);
        }
    }
}
