use super::*;
use serde_json::Value;

/// Reads a fixed binary32 pose or matrix value.
fn number(value: &Value) -> f32 {
    value.as_f64().unwrap() as f32
}

/// Loads the independently calculated ordinary-attack fixtures.
fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures.json")).unwrap()
}

/// Compares a camera or item matrix against a fixed column-major fixture.
fn compare(actual: Mat4, expected: &Value) {
    let expected = Mat4::from_cols_array(&std::array::from_fn(|index| number(&expected[index])));
    assert!(actual.abs_diff_eq(expected, 5e-6), "{actual} != {expected}");
}

#[test]
fn attack_joints_and_shoulders_match_five_progress_points() {
    for case in cases() {
        let pose = legacy_swing_pose(number(&case["progress"]), 0.0);
        assert!((pose.body_yaw - number(&case["body"])).abs() < 2e-6);
        let vector =
            |key: &str| Vec3::from_array(std::array::from_fn(|axis| number(&case[key][axis])));
        assert!(pose.right_rotation.abs_diff_eq(vector("right"), 2e-6));
        assert!(pose.left_rotation.abs_diff_eq(vector("left"), 2e-6));
        let shoulder = vector("shoulder") - Vec3::Y * 22.0;
        assert!(pose.right_shoulder.abs_diff_eq(shoulder, 2e-6));
        assert!(pose.left_shoulder.abs_diff_eq(-shoulder, 2e-6));
    }
}

#[test]
fn looking_down_changes_the_strike_without_changing_the_body_or_shoulders() {
    let neutral = legacy_swing_pose(0.5, 0.0);
    let down = legacy_swing_pose(0.5, 1.0);
    assert!((down.right_rotation.x - neutral.right_rotation.x - 0.75).abs() < 2e-6);
    assert_eq!(down.body_yaw, neutral.body_yaw);
    assert_eq!(down.right_shoulder, neutral.right_shoulder);
    assert_eq!(down.left_rotation, neutral.left_rotation);
}

#[test]
fn bare_arm_and_fallback_held_envelopes_match_fixed_matrices() {
    for case in cases() {
        let progress = number(&case["progress"]);
        compare(legacy_first_person_arm(progress, 1.0), &case["arm"]);
        compare(
            legacy_item_swing(
                progress,
                1.0,
                Mat4::from_rotation_y(BEDROCK_112.arm_base_yaw),
            ),
            &case["held"],
        );
    }
}

#[test]
fn third_person_tool_inherits_the_attack_and_item_parent_once() {
    use crate::equipment::{SpriteGrip, held_sprite_display};
    for case in cases() {
        let pose = legacy_swing_pose(number(&case["progress"]), 0.0);
        let arm = Mat4::from_translation((pose.right_shoulder + Vec3::Y * 22.0) / 16.0)
            * Mat4::from_rotation_y(-pose.right_rotation.y)
            * Mat4::from_rotation_x(-pose.right_rotation.x);
        let item_parent = Mat4::from_translation(Vec3::new(1.0, -7.0, 1.0) / 16.0);
        let display = held_sprite_display(SpriteGrip::HandEquipped);
        let grip = Mat4::from_scale_rotation_translation(
            Vec3::splat(display.scale),
            display.rotation,
            display.translation,
        );
        compare(arm * item_parent * grip, &case["third_item"]);
    }
}
