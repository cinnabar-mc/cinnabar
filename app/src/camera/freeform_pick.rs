//! Touch and classic modes aim interactions along the ray through the finger, not the crosshair.

use bevy::prelude::*;
use client_presentation::{camera::FlyCamera, local_player::InteractionOriginSnapshot};
use client_shell::ui_resources::UiPresentationResource;
use semantic_input::TouchControlMode;

/// Unsent ticks a single frame's pick may cover after a slow frame.
const MAX_PUBLISHED_TICKS: u64 = 16;

/// Where a finger points at the world this frame, in window fractions from the top left.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct FreeformPickPoint(pub(crate) Option<[f32; 2]>);

/// The world direction through `point`, in window fractions, of a camera with `rotation`, a
/// vertical field of view of `fov` radians and width-over-height `aspect`.
fn ray_through(rotation: Quat, fov: f32, aspect: f32, point: [f32; 2]) -> Option<Vec3> {
    let half_height = (fov * 0.5).tan();
    let local = Vec3::new(
        (point[0] * 2.0 - 1.0) * half_height * aspect,
        (1.0 - point[1] * 2.0) * half_height,
        -1.0,
    );
    (rotation * local).try_normalize()
}

/// Reports the touch mode's interaction model and aims this frame's interactions along a finger,
/// after aim assistance so a finger on the world takes priority.
pub(crate) fn publish(
    point: Option<Res<FreeformPickPoint>>,
    presentation: Option<Res<UiPresentationResource>>,
    cameras: Query<(&Transform, &Projection), With<FlyCamera>>,
    mut interaction: ResMut<InteractionOriginSnapshot>,
    mut movement: ResMut<client_interaction::movement::MovementTicker>,
    mut last_tick: Local<u64>,
) {
    movement.set_touch_control_mode(presentation.as_ref().map_or(
        TouchControlMode::Crosshair,
        |view| {
            if view.touch_controls_enabled() {
                view.touch_control_mode()
            } else {
                TouchControlMode::Crosshair
            }
        },
    ));
    let Some(point) = point.and_then(|point| point.0) else {
        // Ticks completed without a finger keep their own aim once one lands.
        if let Some(ray) = interaction.outbound_ray() {
            *last_tick = ray.physics_tick().get();
        }
        return;
    };
    let Ok((transform, Projection::Perspective(projection))) = cameras.single() else {
        return;
    };
    let Some(direction) = ray_through(
        transform.rotation,
        projection.fov,
        projection.aspect_ratio,
        point,
    ) else {
        return;
    };
    interaction.assist_direction(direction);
    let Some(ray) = interaction.outbound_ray() else {
        return;
    };
    let tick = ray.physics_tick().get();
    let first = last_tick
        .saturating_add(1)
        .max(tick.saturating_sub(MAX_PUBLISHED_TICKS));
    for unsent in first..=tick {
        movement.publish_interaction_aim(unsent, Some(direction.to_array()));
    }
    *last_tick = tick;
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOV: f32 = std::f32::consts::FRAC_PI_2;

    fn close(actual: Vec3, expected: Vec3) {
        assert!(
            actual.abs_diff_eq(expected.normalize(), 0.000_1),
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn the_window_centre_is_the_camera_forward() {
        let turned = Quat::from_rotation_y(1.0) * Quat::from_rotation_x(-0.4);
        let ray = ray_through(turned, FOV, 2.0, [0.5, 0.5]).unwrap();
        close(ray, turned * Vec3::NEG_Z);
    }

    /// A 90 degree vertical view reaches 45 degrees at the top edge, and the aspect widens x.
    #[test]
    fn window_edges_follow_the_field_of_view_and_aspect() {
        let forward = Quat::IDENTITY;
        close(
            ray_through(forward, FOV, 2.0, [0.5, 0.0]).unwrap(),
            Vec3::new(0.0, 1.0, -1.0),
        );
        close(
            ray_through(forward, FOV, 2.0, [0.5, 1.0]).unwrap(),
            Vec3::new(0.0, -1.0, -1.0),
        );
        close(
            ray_through(forward, FOV, 2.0, [1.0, 0.5]).unwrap(),
            Vec3::new(2.0, 0.0, -1.0),
        );
        close(
            ray_through(forward, FOV, 2.0, [0.0, 0.5]).unwrap(),
            Vec3::new(-2.0, 0.0, -1.0),
        );
    }

    #[test]
    fn a_degenerate_camera_has_no_ray() {
        assert!(ray_through(Quat::IDENTITY, f32::NAN, 1.0, [0.5, 0.5]).is_none());
    }
}
