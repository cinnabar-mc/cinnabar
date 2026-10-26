use bevy::prelude::{PerspectiveProjection, Projection, Transform, Vec3};

#[test]
fn a_camera_inside_authored_bounds_keeps_the_actor_animating_and_drawing() {
    let bounds = assets::SkinGeometryBounds {
        center: [0.0, 0.9, 0.0],
        half_extents: [0.3, 0.9, 0.3],
    };
    let projection = Projection::Perspective(PerspectiveProjection {
        fov: crate::camera::projection_fov_radians(
            ui::UserSettings::default().video.horizontal_fov_degrees,
        ),
        aspect_ratio: 16.0 / 9.0,
        near: render_api::CAMERA_NEAR_PLANE_BLOCKS,
        ..Default::default()
    });
    for position in [
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
            let up = if forward.y.abs() > 0.99 {
                Vec3::Z
            } else {
                Vec3::Y
            };
            let camera = Transform::from_translation(position).looking_to(forward, up);
            let animation = super::animation_view(&camera, &projection).unwrap();
            let cull = render_model::ActorCullView {
                clip_from_world: projection.get_clip_from_view() * camera.to_matrix().inverse(),
                camera_position: position,
                max_distance: render_model::MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
            };
            assert!(render_model::actor_bounds_are_visible(
                [0.0; 3],
                1.0,
                bounds,
                Some(cull)
            ));
            assert!(animation.admits([0.0; 3], 1.0, true, bounds));
        }
    }
}
