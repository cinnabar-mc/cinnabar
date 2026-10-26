//! Cell overlays as the mesher draws them: extra model geometry in a block's cell, each a
//! registered block state's model turned to one face of the cell. Overlays never cull and never
//! occlude a neighbour, and are lit as the model faces of their cell are.

use assets::{
    DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_BLEND, NetworkIdMode, RuntimeAssets, VisualKind,
};
use block_transform::overlay_face::FaceTurn;

use crate::{
    PackedModelDrawRef, PackedModelRef, PackedQuadLighting, contributors::pack_model_transform,
    lighting::LightingInputs,
};

/// One overlay of a cell, resolved for meshing: the cell within its sub-chunk, the face its
/// model turns to (a Bedrock face id), and the network value of the block state whose model it
/// draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellOverlay {
    pub local: [u8; 3],
    pub face: u8,
    pub network_value: u32,
}

/// The mask of a model turn's quarter turns about y, in the `rotate_cross` sense: turn 1 takes
/// +x to +z. A model reference's transform word holds the turn from bit 12.
pub const MODEL_TURN_Y_MASK: u32 = 3;
/// Where a model turn holds its quarter turns about x, which apply before those about y; each
/// takes +y to +z.
pub const MODEL_TURN_X_SHIFT: u32 = 2;
/// The bits of a model turn that hold its quarter turns about x.
pub const MODEL_TURN_X_MASK: u32 = 3 << MODEL_TURN_X_SHIFT;

/// The model turn that draws an overlay on `face`, a Bedrock face id; none past east (5).
#[must_use]
pub const fn overlay_model_turn(face: u8) -> Option<u32> {
    let Some(turn) = FaceTurn::of(face) else {
        return None;
    };
    // FaceTurn counts y turns that take +x to -z; the model turn's take +x to +z.
    Some(((4 - turn.y as u32) % 4) | (turn.x as u32) << MODEL_TURN_X_SHIFT)
}

/// Turns `position`, in 1/256 block units from the cell's corner, by the model turn `turn`, as
/// the model shader turns template corners: x turns first, then y turns, about the cell's centre.
#[must_use]
pub const fn turn_model_position(mut position: [i16; 3], turn: u32) -> [i16; 3] {
    let mut x_turns = (turn & MODEL_TURN_X_MASK) >> MODEL_TURN_X_SHIFT;
    while x_turns > 0 {
        position = [position[0], 256 - position[2], position[1]];
        x_turns -= 1;
    }
    let [x, y, z] = position;
    match turn & MODEL_TURN_Y_MASK {
        1 => [256 - z, y, x],
        2 => [256 - x, y, 256 - z],
        3 => [z, y, 256 - x],
        _ => [x, y, z],
    }
}

/// Where the overlay pass writes: the mesh's model references, their lighting, and the opaque
/// and blended quads to draw.
pub(crate) struct ModelOutput<'a> {
    pub(crate) refs: &'a mut Vec<PackedModelRef>,
    pub(crate) lighting: &'a mut Vec<PackedQuadLighting>,
    pub(crate) opaque: &'a mut Vec<PackedModelDrawRef>,
    pub(crate) transparent: &'a mut Vec<PackedModelDrawRef>,
}

/// Appends every quad of every overlay's model, turned to its face. An overlay whose state draws
/// no model geometry (a plain cube, or an unknown network value) draws nothing; an unknown value
/// counts as a missing block as terrain's do.
pub(crate) fn mesh_overlays<I: LightingInputs + ?Sized>(
    overlays: &[CellOverlay],
    visuals: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    lighting: &I,
    output: &mut ModelOutput<'_>,
) {
    for overlay in overlays {
        let Some(turn) = overlay_model_turn(overlay.face) else {
            continue;
        };
        let block = visuals.resolve(network_id_mode, overlay.network_value);
        let Some(template_id) = block.model_template() else {
            continue;
        };
        if !matches!(block.kind(), VisualKind::Model | VisualKind::Cross) {
            continue;
        }
        let Some(parts) = assets::model_template_parts(visuals.model_templates(), template_id)
        else {
            continue;
        };
        let emitting = block.light_properties().emission() > 0;
        let local = overlay.local.map(i32::from);
        for (part, template) in parts.iter().enumerate() {
            let part_template = template_id + part as u32;
            let start = template.quad_start as usize;
            let Some(quads) = visuals
                .model_quads()
                .get(start..start.saturating_add(template.quad_count as usize))
            else {
                continue;
            };
            let visible = crate::chunk::build::quad_visibility_mask(template.quad_count);
            let (Ok(lighting_base_index), Ok(model_ref_index)) = (
                u32::try_from(output.lighting.len()),
                u32::try_from(output.refs.len()),
            ) else {
                return;
            };
            if visible == 0 {
                continue;
            }
            let Some(baked) = crate::lighting::bake_template(
                lighting,
                visuals,
                local,
                part_template,
                turn,
                emitting,
            ) else {
                continue;
            };
            output.refs.push(PackedModelRef::new(
                pack_model_transform(overlay.local, turn),
                part_template,
                lighting_base_index,
                visible,
            ));
            output.lighting.extend(baked);
            let mut remaining = visible;
            while remaining != 0 {
                let quad_index = remaining.trailing_zeros();
                remaining &= remaining - 1;
                let Some(quad) = quads.get(quad_index as usize) else {
                    break;
                };
                let draw_ref = PackedModelDrawRef::new(model_ref_index, quad_index);
                if quad.material != DIAGNOSTIC_MATERIAL
                    && visuals.material(quad.material).flags & MATERIAL_FLAG_ALPHA_BLEND != 0
                {
                    output.transparent.push(draw_ref);
                } else {
                    output.opaque.push(draw_ref);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{overlay_model_turn, turn_model_position};
    use block_transform::overlay_face::FACE_TURNS;

    /// The model turn moves every corner where the overlay table turns it.
    #[test]
    fn the_model_turn_of_each_face_matches_the_overlay_table() {
        let corners = [[0, 0, 0], [256, 0, 32], [16, 240, 128], [256, 256, 256]];
        for (face, turn) in FACE_TURNS.into_iter().enumerate() {
            let model = overlay_model_turn(face as u8).unwrap();
            for corner in corners {
                let expected = turn
                    .turn_point(corner.map(f32::from), 256.0)
                    .map(|axis| axis.round() as i16);
                assert_eq!(
                    turn_model_position(corner, model),
                    expected,
                    "face {face} corner {corner:?}"
                );
            }
        }
        assert_eq!(overlay_model_turn(6), None);
    }
}
