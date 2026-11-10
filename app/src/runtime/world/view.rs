//! Render-facing chunk work follows the camera independently of gameplay aim.

use bevy::math::Vec3;
use client_presentation::{camera::CameraSettingsAuthority, local_player::LocalViewPose};

/// Supplies the terrain scheduler's presented heading without changing the player's view pose.
pub(super) fn streaming_forward(
    view: &LocalViewPose,
    settings: &CameraSettingsAuthority,
) -> [f32; 3] {
    (view.camera_rotation(settings) * Vec3::NEG_Z).to_array()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::{EulerRot, Quat};

    /// Look-behind reverses visible terrain priority, while release restores it and aim stays fixed.
    #[test]
    fn streaming_heading_follows_temporary_view_and_restores_without_turning_player() {
        let rotation = Quat::from_euler(EulerRot::YXZ, 0.4, -0.2, 0.);
        let mut view = LocalViewPose::default();
        view.set_rotation(rotation);
        let aim = view.rotation();
        let mut settings = CameraSettingsAuthority::default();
        let forward = Vec3::from_array(streaming_forward(&view, &settings));
        settings.set_view_yaw_offset(Some(180.));
        let behind = Vec3::from_array(streaming_forward(&view, &settings));
        assert!((behind.x * forward.x + behind.z * forward.z) < -0.9);
        assert!((behind.y - forward.y).abs() < 1e-5);
        assert_eq!(view.rotation(), aim);
        settings.set_view_yaw_offset(None);
        assert!(Vec3::from_array(streaming_forward(&view, &settings)).abs_diff_eq(forward, 1e-5));
    }
}
