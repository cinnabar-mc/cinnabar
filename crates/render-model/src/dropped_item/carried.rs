//! Blocks an actor's renderer draws with its body: the block an enderman carries and the pumpkin
//! a snow golem wears.

use glam::{Affine3A, Mat3A, Quat, Vec3, Vec3A};

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

/// Height above the feet of the point a snow golem's pumpkin turns about, at actor scale one.
const SNOW_GOLEM_PUMPKIN_PIVOT_BLOCKS: f32 = 1.3047;
/// Height of the pumpkin's centre above that point while the head rests.
const SNOW_GOLEM_PUMPKIN_ABOVE_PIVOT_BLOCKS: f32 = 0.2930;
const SNOW_GOLEM_PUMPKIN_SCALE: f32 = 0.625;

/// Places the centred pumpkin a snow golem wears. `world_from_model` is the body's rig placement
/// (faces -Z with its right side at +X, feet at the origin), and each head sample is the head
/// part's own `[x, y]` rotation in authored degrees, interpolated by `alpha` like the body.
///
/// The block takes only those two angles and turns about its own point: the native layer ignores
/// the head's Z rotation, position, scale, pivot and parents. The rest pose turns the block half a
/// turn about the vertical, so its south face looks forward.
#[must_use]
pub fn snow_golem_pumpkin_transform(
    world_from_model: [[f32; 4]; 3],
    previous_head: [f32; 2],
    current_head: [f32; 2],
    alpha: f32,
) -> Option<[[f32; 4]; 3]> {
    // Authored X and Y angles turn against the right-hand rule in the mirrored rig frame.
    let rotation = |[x, y]: [f32; 2]| {
        Quat::from_rotation_y(-y.to_radians()) * Quat::from_rotation_x(-x.to_radians())
    };
    let head = rotation(previous_head).slerp(rotation(current_head), alpha.clamp(0.0, 1.0));
    let world_from_block = affine_from_rows(world_from_model)
        * Affine3A::from_translation(Vec3::Y * SNOW_GOLEM_PUMPKIN_PIVOT_BLOCKS)
        * Affine3A::from_quat(head)
        * Affine3A::from_translation(Vec3::Y * SNOW_GOLEM_PUMPKIN_ABOVE_PIVOT_BLOCKS)
        * Affine3A::from_rotation_y(std::f32::consts::PI)
        * Affine3A::from_scale(Vec3::splat(SNOW_GOLEM_PUMPKIN_SCALE));
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

    /// A rig-frame direction in the world of [`facing_south`], whose X and Z axes are negated.
    fn facing_south_axis(direction: Vec3) -> Vec3 {
        Vec3::new(-direction.x, direction.y, -direction.z)
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

    /// The pumpkin on a golem at the origin facing south, with its head held at `head`.
    fn pumpkin_with_head(head: [f32; 2]) -> Affine3A {
        let rows = snow_golem_pumpkin_transform(facing_south([0.0; 3], 1.0), head, head, 0.0);
        affine_from_rows(rows.expect("finite"))
    }

    /// The rig-frame rotation of a head authored at `pitch` and `yaw` degrees, whose rotations the
    /// mirrored frame negates.
    fn authored(pitch: f32, yaw: f32) -> Quat {
        Quat::from_rotation_y(-yaw.to_radians()) * Quat::from_rotation_x(-pitch.to_radians())
    }

    #[test]
    fn a_resting_snow_golem_wears_its_pumpkin_above_the_head_facing_forward() {
        let rows = snow_golem_pumpkin_transform(
            facing_south([10.0, 64.0, 10.0], 1.0),
            [0.0; 2],
            [0.0; 2],
            0.0,
        )
        .expect("finite");
        let transform = affine_from_rows(rows);
        let centre = transform.transform_point3(Vec3::ZERO);
        assert!((centre.x - 10.0).abs() < 1e-5);
        assert!((centre.y - 65.5977).abs() < 1e-3);
        assert!((centre.z - 10.0).abs() < 1e-5);
        assert!((transform.matrix3.y_axis.length() - 0.625).abs() < 1e-5);
        // The golem faces world south here, and the block's south face looks the same way.
        assert!(transform.matrix3.z_axis.z > 0.0);
    }

    #[test]
    fn a_snow_golem_pumpkin_turns_with_the_head_about_its_own_point() {
        let centre = |head| pumpkin_with_head(head).transform_point3(Vec3::ZERO);
        // Yaw turns the block in place.
        let turned = pumpkin_with_head([0.0, 90.0]);
        assert!(
            turned
                .transform_point3(Vec3::ZERO)
                .distance(centre([0.0; 2]))
                < 1e-4
        );
        assert!(turned.matrix3.z_axis.z.abs() < 1e-4);
        // Looking down carries the block forward, toward south here.
        assert!(centre([45.0, 0.0]).z > 0.1);
    }

    #[test]
    fn a_snow_golem_pumpkin_stays_true_to_a_head_turned_past_a_quarter_turn() {
        let up = pumpkin_with_head([20.0, 120.0]).matrix3.y_axis;
        let expected = facing_south_axis(authored(20.0, 120.0) * Vec3::Y);
        assert!(Vec3::from(up).normalize().distance(expected) < 1e-4, "{up}");
    }

    #[test]
    fn a_snow_golem_pumpkin_keeps_a_pitched_head_turned_exactly_sideways() {
        for (pitch, yaw) in [(20.0, 90.0), (20.0, -90.0), (-35.0, 90.0)] {
            let up = pumpkin_with_head([pitch, yaw]).matrix3.y_axis;
            let expected = facing_south_axis(authored(pitch, yaw) * Vec3::Y);
            assert!(
                Vec3::from(up).normalize().distance(expected) < 1e-3,
                "{pitch} {yaw}: {up} != {expected}"
            );
        }
    }

    #[test]
    fn a_snow_golem_pumpkin_interpolates_the_head_samples() {
        let place = |previous, current, alpha| {
            let rows =
                snow_golem_pumpkin_transform(facing_south([0.0; 3], 1.0), previous, current, alpha);
            affine_from_rows(rows.expect("finite")).matrix3
        };
        let quarter = place([0.0; 2], [0.0, 40.0], 0.5);
        assert!(quarter.abs_diff_eq(pumpkin_with_head([0.0, 20.0]).matrix3, 1e-4));
        let clamped = place([0.0; 2], [0.0, 40.0], 2.0);
        assert!(clamped.abs_diff_eq(pumpkin_with_head([0.0, 40.0]).matrix3, 1e-4));
        // A wrapping query crosses the back of the turn with the body, not through the front.
        let wrapped = place([0.0, 170.0], [0.0, -170.0], 0.5);
        assert!(wrapped.abs_diff_eq(pumpkin_with_head([0.0, 180.0]).matrix3, 1e-4));
    }

    #[test]
    fn a_non_finite_head_sample_places_no_pumpkin() {
        let world = facing_south([0.0; 3], 1.0);
        let rest = [0.0; 2];
        assert!(snow_golem_pumpkin_transform(world, rest, rest, f32::NAN).is_none());
        assert!(snow_golem_pumpkin_transform(world, [f32::NAN, 0.0], rest, 1.0).is_none());
    }
}
