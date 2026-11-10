//! Where a cell overlay's item is drawn: flat on its face, as AE2's storage and crafting monitors
//! draw theirs (`init/client/InitStackRenderHandlers.java`, `drawOnBlockFace`): the item as an
//! inventory slot shows it, squashed into the face's plane just in front of the face.

use std::f32::consts::{FRAC_PI_2, PI};

use block_transform::overlay_face::{FaceTurn, OVERLAY_ITEM_LIFT};
use glam::{Mat4, Vec3};

use super::matrix_rows;

/// Java's GUI display pose of a block item, `rotation [30, 225, 0]` and `scale 0.625`, which AE2's
/// monitor draws an item in before flattening it.
const BLOCK_ICON_PITCH_DEGREES: f32 = 30.0;
const BLOCK_ICON_YAW_DEGREES: f32 = 225.0;
const BLOCK_ICON_SCALE: f32 = 0.625;
/// The depth AE2 leaves the item after squashing it into the face: `scale(s, s, 0.001)`.
const FLATTENED_DEPTH: f32 = 0.001;

/// Maps the unit item model to `size` pixels wide on Bedrock face `face` of `block`, its centre
/// `x` pixels right of and `y` pixels above the face's centre as one looks at the face, and
/// [`OVERLAY_ITEM_LIFT`] in front of it. A block item (`block_item`) takes the isometric pose an
/// inventory slot shows it in. `None` for a face past east (5).
#[must_use]
pub fn overlay_item_transform(
    block: [i32; 3],
    face: u8,
    size: f32,
    x: f32,
    y: f32,
    block_item: bool,
) -> Option<[[f32; 4]; 3]> {
    let turn = FaceTurn::of(face)?;
    let pixel = 1.0 / 16.0;
    let cell_centre = Vec3::splat(0.5);
    // FaceTurn's quarter turns about x take +y to +z and those about y take +x to -z, which are
    // glam's positive rotations; the x turns apply first.
    let to_face = Mat4::from_translation(cell_centre)
        * Mat4::from_rotation_y(f32::from(turn.y) * FRAC_PI_2)
        * Mat4::from_rotation_x(f32::from(turn.x) * FRAC_PI_2)
        * Mat4::from_translation(-cell_centre);
    // Authored on the north face, seen from the north: right is west, up is up, out is north.
    let centre = Vec3::new(8.0 - x, 8.0 + y, -OVERLAY_ITEM_LIFT) * pixel;
    // The unit item faces +z, so a half turn about y faces it out of the north face.
    let mut item = Mat4::from_translation(centre)
        * Mat4::from_rotation_y(PI)
        * Mat4::from_scale(Vec3::new(size * pixel, size * pixel, FLATTENED_DEPTH));
    if block_item {
        item = item
            * Mat4::from_rotation_x(BLOCK_ICON_PITCH_DEGREES.to_radians())
            * Mat4::from_rotation_y(BLOCK_ICON_YAW_DEGREES.to_radians())
            * Mat4::from_scale(Vec3::splat(BLOCK_ICON_SCALE));
    }
    let origin = Vec3::new(block[0] as f32, block[1] as f32, block[2] as f32);
    Some(matrix_rows(Mat4::from_translation(origin) * to_face * item))
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::overlay_item_transform;

    /// `point` through the affine rows of an item transform.
    fn apply(rows: [[f32; 4]; 3], point: Vec3) -> Vec3 {
        Vec3::from_array(std::array::from_fn(|row| {
            rows[row][0] * point.x + rows[row][1] * point.y + rows[row][2] * point.z + rows[row][3]
        }))
    }

    /// Each face's item sits centred on that face just outside the cell, its right and up the
    /// overlay table's, its front facing out.
    #[test]
    fn items_lie_on_their_face_turned_as_the_overlay_table_turns_it() {
        // Bedrock faces: down, up, north, south, west, east; their outward normal, and the
        // direction that is up on the face (north for up and down faces, per the table).
        let faces = [
            (Vec3::NEG_Y, Vec3::NEG_Z),
            (Vec3::Y, Vec3::NEG_Z),
            (Vec3::NEG_Z, Vec3::Y),
            (Vec3::Z, Vec3::Y),
            (Vec3::NEG_X, Vec3::Y),
            (Vec3::X, Vec3::Y),
        ];
        for (face, (out, up)) in faces.into_iter().enumerate() {
            let right = up.cross(out);
            let rows =
                overlay_item_transform([3, -2, 5], face as u8, 8.0, 2.0, -1.0, false).unwrap();
            let centre = Vec3::new(3.5, -1.5, 5.5)
                + out * (0.5 + 0.01)
                + right * (2.0 / 16.0)
                + up * (-1.0 / 16.0);
            assert!(
                apply(rows, Vec3::ZERO).abs_diff_eq(centre, 1.0e-5),
                "face {face}: {:?} vs {centre:?}",
                apply(rows, Vec3::ZERO)
            );
            let item_up = apply(rows, Vec3::Y) - apply(rows, Vec3::ZERO);
            let item_right = apply(rows, Vec3::X) - apply(rows, Vec3::ZERO);
            let item_front = apply(rows, Vec3::Z) - apply(rows, Vec3::ZERO);
            assert!(item_up.abs_diff_eq(up * 0.5, 1.0e-5), "face {face} up");
            assert!(
                item_right.abs_diff_eq(right * 0.5, 1.0e-5),
                "face {face} right"
            );
            assert!(
                item_front.normalize().abs_diff_eq(out, 1.0e-5) && item_front.length() < 0.01,
                "face {face} front"
            );
        }
        assert!(overlay_item_transform([0; 3], 6, 8.0, 0.0, 0.0, false).is_none());
    }
}
