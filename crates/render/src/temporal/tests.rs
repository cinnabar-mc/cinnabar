use super::*;
use bevy::math::UVec4;
use render_scene::temporal::TemporalUpscaler;

const NEAR: f32 = 0.025;

fn request(reset_epoch: u64) -> TemporalUpscaling {
    TemporalUpscaling {
        upscaler: TemporalUpscaler::Dlss,
        reset_epoch,
        delta_seconds: 1.0 / 60.0,
    }
}

fn history(position: [f32; 3], reset_epoch: u64) -> CameraHistory {
    let clip_from_view = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        70f32.to_radians(),
        16.0 / 9.0,
        NEAR,
    );
    let world_from_view = Mat4::from_translation(position.into());
    CameraHistory::new(
        clip_from_view,
        world_from_view,
        UVec4::new(0, 0, 1920, 1080),
        reset_epoch,
    )
}

/// The previous NDC position of the current pixel at `ndc` and reverse-Z depth `z`.
fn previous_ndc(matrix: Mat4, ndc: [f32; 2], z: f32) -> [f32; 2] {
    let previous = matrix * Vec4::new(ndc[0], ndc[1], z, 1.0);
    [previous.x / previous.w, previous.y / previous.w]
}

#[test]
fn the_first_frame_and_camera_cuts_reset_history() {
    let current = history([0.0, 64.0, 0.0], 1);
    assert_eq!(
        reprojection(None, current, &request(1)),
        (false, Mat4::IDENTITY)
    );
    assert_eq!(
        reprojection(Some(history([0.0, 64.0, 0.0], 0)), current, &request(1)),
        (false, Mat4::IDENTITY)
    );
    // A teleport-sized jump is a cut even within one epoch.
    assert_eq!(
        reprojection(Some(history([100.0, 64.0, 0.0], 1)), current, &request(1)),
        (false, Mat4::IDENTITY)
    );
}

#[test]
fn a_still_camera_maps_every_pixel_to_itself() {
    let current = history([0.0, 64.0, 0.0], 1);
    let (continuous, matrix) = reprojection(Some(current), current, &request(1));
    assert!(continuous);
    for (ndc, z) in [([0.0, 0.0], 0.5), ([0.7, -0.4], 0.01), ([-0.9, 0.9], 0.0)] {
        let previous = previous_ndc(matrix, ndc, z);
        assert!((previous[0] - ndc[0]).abs() < 1e-5 && (previous[1] - ndc[1]).abs() < 1e-5);
    }
}

#[test]
fn moving_right_places_near_surfaces_further_right_last_frame_and_leaves_the_sky() {
    let previous = history([0.0, 64.0, 0.0], 1);
    let current = history([0.5, 64.0, 0.0], 1);
    let (continuous, matrix) = reprojection(Some(previous), current, &request(1));
    assert!(continuous);
    // A surface 5 blocks ahead: reverse-Z depth is near / distance.
    let near_surface = previous_ndc(matrix, [0.0, 0.0], NEAR / 5.0);
    assert!(near_surface[0] > 0.01, "{near_surface:?}");
    assert!(near_surface[1].abs() < 1e-5);
    // The sky is infinitely far, so translation does not move it.
    let sky = previous_ndc(matrix, [0.3, 0.2], 0.0);
    assert!(
        (sky[0] - 0.3).abs() < 1e-5 && (sky[1] - 0.2).abs() < 1e-5,
        "{sky:?}"
    );
}

#[test]
fn long_frame_gaps_reset_history() {
    let previous = history([0.0, 64.0, 0.0], 1);
    let stalled = TemporalUpscaling {
        delta_seconds: 1.0,
        ..request(1)
    };
    assert!(!reprojection(Some(previous), previous, &stalled).0);
}
