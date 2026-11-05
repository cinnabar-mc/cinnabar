//! Pure placement invariants shared by world equipment and UI previews.

use super::{ItemDisplay, SpriteGrip, attach_to_bone, held_block_display, held_sprite_display};
use crate::RenderBoneTransform;
use glam::{Quat, Vec3};

/// Builds a hand bone with identity rotation for placement assertions.
fn bone(translation: [f32; 3], scale: f32) -> RenderBoneTransform {
    RenderBoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [translation[0], translation[1], translation[2], scale],
        axis_scale: crate::UNIT_AXIS_SCALE,
    }
}

/// Places one 16-texel sprite corner in the hand frame.
fn icon_point(display: ItemDisplay, column: f32, row: f32) -> Vec3 {
    let local = Vec3::new(-column / 16.0, 1.0 - row / 16.0, 0.0);
    display.translation + display.rotation * (local * display.scale)
}

#[test]
fn attach_with_identity_display_passes_the_hand_pose_through() {
    let display = ItemDisplay {
        rotation: Quat::IDENTITY,
        translation: Vec3::ZERO,
        scale: 1.0,
        axis_scale: Vec3::ONE,
    };
    let attached = attach_to_bone(bone([0.25, 0.5, -0.75], 1.0), display).unwrap();
    assert_eq!(attached.translation_scale, [0.25, 0.5, -0.75, 1.0]);
    assert_eq!(attached.rotation, [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn attach_scales_the_display_offset_by_the_hand_scale_and_hides_with_it() {
    let display = ItemDisplay {
        rotation: Quat::IDENTITY,
        translation: Vec3::new(0.0, 1.0, 0.0),
        scale: 0.5,
        axis_scale: Vec3::ONE,
    };
    let attached = attach_to_bone(bone([0.0, 2.0, 0.0], 2.0), display).unwrap();
    assert_eq!(attached.translation_scale, [0.0, 4.0, 0.0, 1.0]);
    let hidden = attach_to_bone(bone([1.0, 1.0, 1.0], 0.0), display).unwrap();
    assert_eq!(hidden.translation_scale[3], 0.0);
    let mut bad = bone([0.0; 3], 1.0);
    bad.rotation = [0.0; 4];
    assert!(attach_to_bone(bad, display).is_none());
}

#[test]
fn hand_rotation_turns_the_display_offset() {
    let mut hand = bone([0.0; 3], 1.0);
    hand.rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2).to_array();
    let display = ItemDisplay {
        rotation: Quat::IDENTITY,
        translation: Vec3::X,
        scale: 1.0,
        axis_scale: Vec3::ONE,
    };
    let attached = attach_to_bone(hand, display).unwrap();
    assert!((attached.translation_scale[1] - 1.0).abs() < 1e-5);
    assert!(attached.translation_scale[0].abs() < 1e-5);
}

#[test]
fn held_item_placements_follow_the_reference_scales() {
    let sprite = held_sprite_display(SpriteGrip::Flat);
    let sword = held_sprite_display(SpriteGrip::HandEquipped);
    let rod = held_sprite_display(SpriteGrip::Rod);
    let block = held_block_display();
    assert!((sprite.scale - 0.5625).abs() < 1e-5, "{}", sprite.scale);
    assert!((sword.scale - 0.9375).abs() < 1e-5, "{}", sword.scale);
    assert!((rod.scale - 0.9375).abs() < 1e-5, "{}", rod.scale);
    assert!((block.scale - 0.375).abs() < 1e-5, "{}", block.scale);
}

#[test]
fn block_face_rects_tile_the_three_by_two_sheet() {
    let rects = super::blocks::face_rects([0.0, 0.0, 0.75, 0.5]);
    assert_eq!(rects[0], [0.0, 0.0, 0.25, 0.25]);
    assert_eq!(rects[5], [0.5, 0.25, 0.75, 0.5]);
}

#[test]
fn attachable_bone_uses_bound_model_origin_and_mirrored_literal_offset() {
    use super::{BoneChannels, attach};
    let hand = bone([1.0, 1.0, 1.0], 2.0);
    let channels = BoneChannels {
        translation: [16.0, 8.0, -16.0],
        rotation: [0.0; 3],
        scale: [1.0, -1.0, -1.0],
    };
    let posed = attach(hand, [0.0, 1.5, 0.0], channels, true).unwrap();
    // Pivot and offset are in the hand frame, so the hand scale (2) stretches them.
    assert_eq!(
        posed.translation_scale,
        [1.0 - 2.0, 1.0 + 2.0 * 0.5, 1.0 - 2.0, 2.0]
    );
    assert_eq!(posed.axis_scale, [1.0, -1.0, -1.0, 1.0]);
    let mut broken = hand;
    broken.rotation = [0.0; 4];
    assert!(attach(broken, [0.0; 3], channels, true).is_none());
}

#[test]
fn third_person_bound_root_stays_at_the_hand_under_rotation_and_scale() {
    use super::{BoneChannels, attach};
    let mut hand = bone([2.0, 0.75, -3.0], 0.5);
    hand.rotation = Quat::from_rotation_z(0.7).to_array();
    let pivot = [0.0, assets::gui_item::SHIELD_MODEL_PART_HEIGHT / 16.0, 0.0];
    let posed = attach(hand, pivot, BoneChannels::default(), true).unwrap();
    assert_eq!(posed.translation_scale, hand.translation_scale);
    assert!(
        posed
            .rotation
            .iter()
            .zip(hand.rotation)
            .all(|(actual, expected)| (actual - expected).abs() < 1e-6)
    );
}

#[test]
fn third_person_sword_points_forward_and_up_from_a_hanging_arm() {
    let sword = held_sprite_display(SpriteGrip::HandEquipped);
    let handle = icon_point(sword, 1.0, 15.0);
    let tip = icon_point(sword, 15.0, 1.0);
    let blade = tip - handle;
    assert!(blade.z < -0.5 && blade.y > 0.0, "{blade}");
}

/// Rod icons run from a bottom-left handle to a top-right tip, with the line drawn below the tip.
/// Vanilla turns the rod over so the line hangs from the forward tip; the sword grip raised it.
#[test]
fn third_person_rod_points_forward_with_its_line_hanging_down() {
    let line_below_tip = |grip| {
        let display = held_sprite_display(grip);
        let handle = icon_point(display, 1.0, 15.0);
        let tip = icon_point(display, 15.0, 1.0);
        let line = icon_point(display, 15.0, 12.0);
        let rod = tip - handle;
        assert!(rod.z < -0.5 && rod.y > 0.0, "{grip:?} {rod}");
        (line - tip).y < 0.0
    };
    assert!(line_below_tip(SpriteGrip::Rod));
    assert!(!line_below_tip(SpriteGrip::HandEquipped));
}

/// Pins each hand's rod placement on its item bone: the main hand holds the rod lower and further
/// back than the off hand, as vanilla's two rod offsets do.
#[test]
fn third_person_rod_placement_differs_per_hand() {
    use super::held_sprite_display_for_hand;
    let near = |actual: Vec3, expected: [f32; 3]| {
        assert!(
            actual.distance(Vec3::from_array(expected)) < 1e-3,
            "{actual} != {expected:?}"
        );
    };
    for (off_hand, handle, tip) in [
        (false, [-0.012, -0.0841, 0.1928], [-0.0466, 0.0184, -0.9623]),
        (true, [0.113, 0.1252, 0.1842], [0.0784, 0.2277, -0.9709]),
    ] {
        let display = held_sprite_display_for_hand(SpriteGrip::Rod, off_hand);
        near(icon_point(display, 1.0, 15.0), handle);
        near(icon_point(display, 15.0, 1.0), tip);
    }
}

#[test]
fn rod_grip_wins_over_the_hand_equipped_flag() {
    assert_eq!(SpriteGrip::new(true, true), SpriteGrip::Rod);
    assert_eq!(SpriteGrip::new(false, true), SpriteGrip::Rod);
    assert_eq!(SpriteGrip::new(true, false), SpriteGrip::HandEquipped);
    assert_eq!(SpriteGrip::new(false, false), SpriteGrip::Flat);
    for identifier in [
        "minecraft:fishing_rod",
        "minecraft:carrot_on_a_stick",
        "minecraft:warped_fungus_on_a_stick",
    ] {
        let grip = assets::item::grip::builtin_item_grip(identifier);
        assert_eq!(SpriteGrip::from(grip), SpriteGrip::Rod, "{identifier}");
    }
    let sword = assets::item::grip::builtin_item_grip("minecraft:diamond_sword");
    assert_eq!(SpriteGrip::from(sword), SpriteGrip::HandEquipped);
}

#[test]
fn unbound_attachable_preserves_its_hand_relative_origin_under_rotation_and_scale() {
    use super::{BoneChannels, attach};
    let mut hand = bone([2.0, 0.75, -3.0], 0.5);
    hand.rotation = Quat::from_rotation_z(0.7).to_array();
    for pivot in [[0.0; 3], [0.25, 1.5, -0.125]] {
        let posed = attach(hand, pivot, BoneChannels::default(), false).unwrap();
        let expected = Vec3::from_array([2.0, 0.75, -3.0])
            + Quat::from_rotation_z(0.7) * (Vec3::from_array(pivot) * 0.5);
        for axis in 0..3 {
            assert!((posed.translation_scale[axis] - expected[axis]).abs() < 1e-6);
        }
        assert_eq!(posed.translation_scale[3], 0.5);
    }
}
