//! Blocks an actor's renderer draws with its body, such as the block an enderman carries.

use glam::{Affine3A, Mat3A, Vec3, Vec3A};

/// Model-origin height per unit of actor scale in the vanilla entity frame (24 pixels).
const MODEL_ORIGIN_BLOCKS: f32 = 1.5;
/// Vanilla's carried-block offset in the entity frame, whose Y axis points down from the
/// model origin and whose front is -Z.
const ENDERMAN_BLOCK_OFFSET: [f32; 3] = [0.0, 0.6875, -0.75];
const ENDERMAN_BLOCK_TILT_DEGREES: f32 = 20.0;
const ENDERMAN_BLOCK_YAW_DEGREES: f32 = 45.0;
/// Vanilla's carried-block scale in the entity frame; the X and Y flips undo the frame's.
const ENDERMAN_BLOCK_SCALE: [f32; 3] = [-0.5, -0.5, 0.5];

/// Places the centred block an enderman carries. `world_from_model` is the body's rig placement
/// (faces -Z with its right side at +X, feet at the origin), which includes `actor_scale`.
///
/// Vanilla draws the block in the entity frame after undoing the actor scale, so the block keeps
/// its half-block size while its anchor follows the scaled model origin. The rig frame is the
/// entity frame with X and Y negated about that origin.
#[must_use]
pub fn enderman_carried_block_transform(
    world_from_model: [[f32; 4]; 3],
    actor_scale: f32,
) -> Option<[[f32; 4]; 3]> {
    if !actor_scale.is_finite() || actor_scale <= 0.0 {
        return None;
    }
    let world_from_rig = affine_from_rows(world_from_model);
    let flip = Affine3A::from_scale(Vec3::new(-1.0, -1.0, 1.0));
    let rig_from_entity =
        Affine3A::from_translation(Vec3::Y * MODEL_ORIGIN_BLOCKS * actor_scale) * flip;
    let entity_from_block = Affine3A::from_translation(Vec3::from(ENDERMAN_BLOCK_OFFSET))
        * Affine3A::from_rotation_x(ENDERMAN_BLOCK_TILT_DEGREES.to_radians())
        * Affine3A::from_rotation_y(ENDERMAN_BLOCK_YAW_DEGREES.to_radians())
        * Affine3A::from_scale(Vec3::from(ENDERMAN_BLOCK_SCALE));
    let world_from_block = world_from_rig
        * Affine3A::from_scale(Vec3::splat(actor_scale.recip()))
        * rig_from_entity
        * entity_from_block;
    world_from_block
        .is_finite()
        .then(|| rows_from_affine(world_from_block))
}

/// Reads three affine rows `[x, y, z, translation]` as a transform.
fn affine_from_rows(rows: [[f32; 4]; 3]) -> Affine3A {
    let column = |index: usize| Vec3A::new(rows[0][index], rows[1][index], rows[2][index]);
    Affine3A {
        matrix3: Mat3A::from_cols(column(0), column(1), column(2)),
        translation: column(3),
    }
}

/// Writes a transform as three affine rows `[x, y, z, translation]`.
fn rows_from_affine(transform: Affine3A) -> [[f32; 4]; 3] {
    let columns = [
        transform.matrix3.x_axis,
        transform.matrix3.y_axis,
        transform.matrix3.z_axis,
        transform.translation,
    ];
    std::array::from_fn(|row| std::array::from_fn(|column| columns[column][row]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rig placement at `position` facing world +Z (Minecraft yaw 0), scaled about the feet.
    fn facing_south(position: [f32; 3], scale: f32) -> [[f32; 4]; 3] {
        [
            [-scale, 0.0, 0.0, position[0]],
            [0.0, scale, 0.0, position[1]],
            [0.0, 0.0, -scale, position[2]],
        ]
    }

    fn apply(rows: [[f32; 4]; 3], point: [f32; 3]) -> Vec3 {
        affine_from_rows(rows).transform_point3(Vec3::from(point))
    }

    #[test]
    fn enderman_block_sits_half_size_in_front_of_the_body_at_hand_height() {
        let rows = enderman_carried_block_transform(facing_south([10.0, 64.0, 10.0], 1.0), 1.0)
            .expect("finite placement");
        let centre = apply(rows, [0.0; 3]);
        assert!(
            centre.abs_diff_eq(Vec3::new(10.0, 64.8125, 10.75), 1e-5),
            "{centre}"
        );
        let axes = affine_from_rows(rows).matrix3;
        for axis in [axes.x_axis, axes.y_axis, axes.z_axis] {
            assert!((axis.length() - 0.5).abs() < 1e-5, "{axis}");
        }
        assert!(axes.determinant() > 0.0, "faces keep their winding");
        // The block's top tips 20 degrees forward, away from the body.
        let up = Vec3::from(axes.y_axis).normalize();
        assert!((up.y - 20.0_f32.to_radians().cos()).abs() < 1e-5, "{up}");
        assert!((up.z - 20.0_f32.to_radians().sin()).abs() < 1e-5, "{up}");
    }

    #[test]
    fn actor_scale_moves_the_anchor_without_resizing_the_block() {
        let rows = enderman_carried_block_transform(facing_south([0.0, 0.0, 0.0], 2.0), 2.0)
            .expect("finite placement");
        let centre = apply(rows, [0.0; 3]);
        assert!(
            centre.abs_diff_eq(Vec3::new(0.0, 3.0 - 0.6875, 0.75), 1e-5),
            "{centre}"
        );
        let axes = affine_from_rows(rows).matrix3;
        assert!((Vec3::from(axes.x_axis).length() - 0.5).abs() < 1e-5);
        assert!(enderman_carried_block_transform(facing_south([0.0; 3], 1.0), 0.0).is_none());
    }
}
