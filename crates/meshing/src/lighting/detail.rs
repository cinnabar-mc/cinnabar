//! Detail quads that keep the vertex colour and light of the cuboid face drawn before them.

use assets::{MODEL_QUAD_FLAG_FACE_MASK, ModelQuad};

use crate::{Face, PackedQuadLighting};

/// Native tessellation draws some detail quads, such as bamboo leaves, lantern handles and sea
/// pickle tips, straight after a cuboid without setting a new vertex colour or light. Each run
/// of [`assets::MODEL_QUAD_FLAG_INHERITED_SHADE`] quads therefore repeats the final vertex of
/// the last face that cuboid drew, and takes that face's coefficient through
/// [`crate::LIGHT_SAMPLE_SHADE_FACE_SHIFT`]. A run without a drawn source face keeps its bake.
pub(super) fn inherit_detail_lighting(
    quads: &[ModelQuad],
    rotation: u32,
    visible: u32,
    lighting: &mut [PackedQuadLighting],
) {
    let inherits = |quad: &ModelQuad| quad.flags & assets::MODEL_QUAD_FLAG_INHERITED_SHADE != 0;
    let mut start = 0;
    while let Some(offset) = quads[start..].iter().position(inherits) {
        let run = start + offset;
        let end = quads[run..]
            .iter()
            .position(|quad| !inherits(quad))
            .map_or(quads.len(), |length| run + length);
        if let Some(sample) = last_drawn_face_sample(&quads[..run], lighting, rotation, visible)
            && let Some(targets) = lighting.get_mut(run..end)
        {
            targets.fill(PackedQuadLighting::new([sample; 4]));
        }
        start = end;
    }
}

/// Faces in the order native cuboid tessellation draws them.
const NATIVE_FACE_ORDER: [Face; 6] = [
    Face::NegativeY,
    Face::PositiveY,
    Face::NegativeZ,
    Face::PositiveZ,
    Face::NegativeX,
    Face::PositiveX,
];

/// The packed sample native tessellation leaves behind after the cuboid that ends `quads`: the
/// distinct faced quads at its end. Of the faces still drawn after culling, the last in native
/// order supplies its final vertex, tagged with that face as the shade face.
fn last_drawn_face_sample(
    quads: &[ModelQuad],
    lighting: &[PackedQuadLighting],
    rotation: u32,
    visible: u32,
) -> Option<u16> {
    let mut seen = 0_u32;
    let mut last: Option<(usize, Face, usize)> = None;
    for (index, quad) in quads.iter().enumerate().rev() {
        let face_bit = 1 << (quad.flags & MODEL_QUAD_FLAG_FACE_MASK);
        if quad.flags & assets::MODEL_QUAD_FLAG_INHERITED_SHADE != 0
            || face_bit == 1
            || seen & face_bit != 0
        {
            break;
        }
        seen |= face_bit;
        let drawn = u32::try_from(index)
            .ok()
            .and_then(|index| 1_u32.checked_shl(index))
            .is_some_and(|bit| visible & bit != 0);
        let Some(face) = super::model_quad_face(*quad, rotation).filter(|_| drawn) else {
            continue;
        };
        let order = NATIVE_FACE_ORDER
            .iter()
            .position(|&native| native == face)?;
        if last.is_none_or(|(latest, _, _)| order > latest) {
            last = Some((order, face, index));
        }
    }
    let (_, face, index) = last?;
    let positions = quads[index]
        .positions
        .map(|position| crate::overlays::turn_model_position(position, rotation));
    let corner = (0..4).max_by_key(|&corner| native_final_vertex_rank(face, positions[corner]))?;
    let sample = lighting.get(index)?.samples()[corner];
    Some(sample | (model_face_id(face) << crate::LIGHT_SAMPLE_SHADE_FACE_SHIFT))
}

/// Orders a face's corners so that the corner native tessellation emits last ranks highest:
/// down ends at +X+Z, up at -X+Z, north at -X-Y, south at +X+Y, west at -Y+Z and east at +Y+Z.
const fn native_final_vertex_rank(face: Face, position: [i16; 3]) -> (i32, i32) {
    let [x, y, z] = [position[0] as i32, position[1] as i32, position[2] as i32];
    match face {
        Face::NegativeY => (x, z),
        Face::PositiveY => (-x, z),
        Face::NegativeZ => (-x, -y),
        Face::PositiveZ => (x, y),
        Face::NegativeX => (-y, z),
        Face::PositiveX => (y, z),
    }
}

/// Model-carrier face id of `face`, as [`MODEL_QUAD_FLAG_FACE_MASK`] encodes it.
const fn model_face_id(face: Face) -> u16 {
    match face {
        Face::NegativeY => 1,
        Face::PositiveY => 2,
        Face::NegativeX => 3,
        Face::PositiveX => 4,
        Face::NegativeZ => 5,
        Face::PositiveZ => 6,
    }
}

#[cfg(test)]
#[path = "detail_tests.rs"]
mod tests;
