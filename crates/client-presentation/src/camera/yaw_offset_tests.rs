//! Relative camera headings preserve the player and first-person body visibility.

use super::*;
use semantic_input::PerspectiveMode;

/// A half-turn changes the presented heading, with no detached third-person body or hand.
#[test]
fn yaw_offset_reverses_only_the_presented_heading_and_restores() {
    let mut settings = CameraSettingsAuthority::default();
    let player = Quat::from_euler(EulerRot::YXZ, 0.6, -0.2, 0.0);
    let forward = player * Vec3::NEG_Z;
    settings.set_mod_perspective(Some(PerspectiveMode::FirstPerson));
    settings.set_view_yaw_offset(Some(180.));
    let backward = settings.offset_view_rotation(player) * Vec3::NEG_Z;
    assert!((backward.x + forward.x).abs() < 1e-5);
    assert!((backward.z + forward.z).abs() < 1e-5);
    assert!((backward.y - forward.y).abs() < 1e-5);
    assert_eq!(
        settings.rendered_perspective(),
        PerspectiveMode::FirstPerson
    );
    assert!(!hand_first_person(Some(&settings), None));
    settings.set_view_yaw_offset(None);
    assert!(
        settings
            .offset_view_rotation(player)
            .abs_diff_eq(player, 1e-5)
    );
    assert!(hand_first_person(Some(&settings), None));
    settings.set_view_yaw_offset(Some(f32::NAN));
    assert!(settings.view_aligned());
}
