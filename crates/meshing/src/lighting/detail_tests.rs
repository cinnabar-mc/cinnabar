use assets::{MODEL_QUAD_FLAG_INHERITED_SHADE, ModelQuad};

use super::inherit_detail_lighting;
use crate::lighting::cube_face_positions;
use crate::{Face, LIGHT_SAMPLE_SHADE_FACE_SHIFT, PackedQuadLighting};

/// Template order of a cuboid's faces: west, east, down, up, north, south.
const TEMPLATE_FACES: [(Face, u32); 6] = [
    (Face::NegativeX, 3),
    (Face::PositiveX, 4),
    (Face::NegativeY, 1),
    (Face::PositiveY, 2),
    (Face::NegativeZ, 5),
    (Face::PositiveZ, 6),
];

fn quad(positions: [[i16; 3]; 4], flags: u32) -> ModelQuad {
    ModelQuad {
        positions,
        uvs: [[0; 2]; 4],
        material: 0,
        flags,
    }
}

/// Two full cuboids followed by `details` inherited detail quads.
fn template(details: usize) -> Vec<ModelQuad> {
    let cuboid = TEMPLATE_FACES.map(|(face, id)| quad(cube_face_positions(face), id));
    let detail = quad([[128, 0, 128]; 4], MODEL_QUAD_FLAG_INHERITED_SHADE);
    cuboid
        .into_iter()
        .chain(cuboid)
        .chain(std::iter::repeat_n(detail, details))
        .collect()
}

/// Lighting whose sample identifies its quad and corner: `quad * 4 + corner`.
fn identified_lighting(count: usize) -> Vec<PackedQuadLighting> {
    (0..count)
        .map(|quad| {
            PackedQuadLighting::new(std::array::from_fn(|corner| (quad * 4 + corner) as u16))
        })
        .collect()
}

fn inherited(quads: &[ModelQuad], rotation: u32, visible: u32) -> Vec<PackedQuadLighting> {
    let mut lighting = identified_lighting(quads.len());
    inherit_detail_lighting(quads, rotation, visible, &mut lighting);
    lighting
}

fn shaded(quad: usize, corner: usize, face_id: u16) -> PackedQuadLighting {
    PackedQuadLighting::new(
        [(quad * 4 + corner) as u16 | face_id << LIGHT_SAMPLE_SHADE_FACE_SHIFT; 4],
    )
}

#[test]
fn details_repeat_the_last_cuboid_east_face_final_vertex() {
    let quads = template(3);
    let lighting = inherited(&quads, 0, u32::MAX);
    // East of the second cuboid ends at its top +Z corner.
    let east = 7;
    let corner = (0..4)
        .find(|&corner| quads[east].positions[corner] == [256, 256, 256])
        .unwrap();
    assert!(
        lighting[12..]
            .iter()
            .all(|&sample| sample == shaded(east, corner, 4))
    );
    assert_eq!(
        lighting[..12],
        identified_lighting(12)[..],
        "cuboid faces keep their bake"
    );
}

#[test]
fn a_culled_east_face_hands_the_shade_to_west() {
    let quads = template(1);
    let lighting = inherited(&quads, 0, !(1 << 7));
    // West ends at its bottom +Z corner; the first cuboid's east face is not a source.
    let west = 6;
    let corner = (0..4)
        .find(|&corner| quads[west].positions[corner] == [0, 0, 256])
        .unwrap();
    assert_eq!(lighting[12], shaded(west, corner, 3));
}

#[test]
fn turned_models_take_the_world_east_face() {
    let quads = template(1);
    // A quarter turn about y carries the template's north face to world east.
    let lighting = inherited(&quads, 1, u32::MAX);
    let north = 10;
    let corner = (0..4)
        .find(|&corner| {
            crate::overlays::turn_model_position(quads[north].positions[corner], 1)
                == [256, 256, 256]
        })
        .unwrap();
    assert_eq!(lighting[12], shaded(north, corner, 4));
}

#[test]
fn details_without_a_drawn_cuboid_keep_their_bake() {
    let mut quads = template(2);
    quads.drain(..12);
    assert_eq!(inherited(&quads, 0, u32::MAX), identified_lighting(2));
    let quads = template(1);
    assert_eq!(inherited(&quads, 0, 1 << 12), identified_lighting(13));
}
