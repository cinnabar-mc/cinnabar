use super::actor_bounds_are_visible;
use crate::{ActorCullView, MAX_ACTOR_RENDER_DISTANCE_BLOCKS};
use assets::SkinGeometryBounds;
use glam::Vec3;

/// Builds the game's reverse-Z perspective at a representative 60-degree FOV.
fn view(camera: Vec3, forward: Vec3) -> ActorCullView {
    let up = if forward.y.abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    ActorCullView {
        clip_from_world: glam::camera::rh::proj::directx::perspective_infinite_reverse(
            60_f32.to_radians(),
            16.0 / 9.0,
            render_api::CAMERA_NEAR_PLANE_BLOCKS,
        ) * glam::camera::rh::view::look_to_mat4(camera, forward, up),
        camera_position: camera,
        max_distance: MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
    }
}

#[test]
fn a_player_box_containing_the_camera_stays_visible_in_every_direction() {
    let bounds = SkinGeometryBounds {
        center: [0.0, 0.9, 0.0],
        half_extents: [0.3, 0.9, 0.3],
    };
    for camera in [
        Vec3::new(0.0, 1.62, 0.0),
        Vec3::new(0.29, 1.62, 0.29),
        Vec3::new(-0.29, 1.62, -0.29),
        Vec3::new(0.0, 1.79, 0.0),
        Vec3::new(0.0, 0.01, 0.0),
    ] {
        for forward in [
            Vec3::X,
            Vec3::NEG_X,
            Vec3::Y,
            Vec3::NEG_Y,
            Vec3::Z,
            Vec3::NEG_Z,
        ] {
            assert!(
                actor_bounds_are_visible([0.0; 3], 1.0, bounds, Some(view(camera, forward))),
                "camera={camera:?} forward={forward:?}"
            );
        }
    }
}

#[test]
fn camera_containment_uses_the_positioned_and_scaled_authored_box() {
    let feet = [12.0, 64.0, -8.0];
    let bounds = SkinGeometryBounds {
        center: [1.0, 0.5, -1.0],
        half_extents: [0.2, 0.3, 0.2],
    };
    let (_, high) = bounds.at(feet, 3.0);
    let camera = Vec3::from_array(high) - Vec3::splat(0.01);
    for forward in [Vec3::X, Vec3::Y, Vec3::Z] {
        assert!(actor_bounds_are_visible(
            feet,
            3.0,
            bounds,
            Some(view(camera, forward))
        ));
    }
}

#[test]
fn camera_containment_preserves_frustum_near_plane_and_distance_rejection() {
    let camera = Vec3::new(0.0, 1.0, 0.0);
    let view = view(camera, Vec3::NEG_Z);
    for feet in [
        [0.0, 0.0, 4.0],
        [20.0, 0.0, -4.0],
        [-20.0, 0.0, -4.0],
        [0.0, 20.0, -4.0],
        [0.0, -20.0, -4.0],
        [0.0, 0.0, -MAX_ACTOR_RENDER_DISTANCE_BLOCKS - 1.0],
    ] {
        assert!(
            !actor_bounds_are_visible(feet, 1.0, Default::default(), Some(view)),
            "{feet:?}"
        );
    }
    assert!(actor_bounds_are_visible(
        [0.0, 0.0, -4.0],
        1.0,
        Default::default(),
        Some(view)
    ));
    let tiny = SkinGeometryBounds {
        center: [0.0; 3],
        half_extents: [0.001; 3],
    };
    let before_near =
        (camera + Vec3::NEG_Z * render_api::CAMERA_NEAR_PLANE_BLOCKS * 0.5).to_array();
    assert!(!actor_bounds_are_visible(
        before_near,
        1.0,
        tiny,
        Some(view)
    ));
    let containing = SkinGeometryBounds {
        center: [0.0; 3],
        half_extents: [2.0; 3],
    };
    assert!(!actor_bounds_are_visible(
        [1.0, 0.0, 0.0],
        1.0,
        containing,
        Some(ActorCullView {
            max_distance: 0.5,
            ..view
        })
    ));
}
