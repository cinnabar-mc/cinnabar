use glam::{Vec3, Vec4};

use crate::ActorCullView;

/// Tests an authored model visibility box against the same distance and frustum as default actors.
#[must_use]
pub fn actor_bounds_are_visible(
    feet: [f32; 3],
    scale: f32,
    bounds: assets::SkinGeometryBounds,
    view: Option<ActorCullView>,
) -> bool {
    let Some(view) = view.filter(ActorCullView::is_valid) else {
        return true;
    };
    let feet = Vec3::from_array(feet);
    if !view.contains_distance(feet.to_array()) {
        return false;
    }
    let (low, high) = bounds.at(feet.to_array(), scale);
    // A box containing the eye must survive even when its forward face is before the near plane.
    if (0..3).all(|axis| {
        low[axis] <= view.camera_position[axis] && view.camera_position[axis] <= high[axis]
    }) {
        return true;
    }
    let corners: [Vec4; 8] = std::array::from_fn(|index| {
        let point = Vec3::from_array(std::array::from_fn(|axis| {
            if index & (1 << axis) == 0 {
                low[axis]
            } else {
                high[axis]
            }
        }));
        view.clip_from_world * point.extend(1.0)
    });
    !outside_clip_plane(&corners, |clip| clip.x < -clip.w)
        && !outside_clip_plane(&corners, |clip| clip.x > clip.w)
        && !outside_clip_plane(&corners, |clip| clip.y < -clip.w)
        && !outside_clip_plane(&corners, |clip| clip.y > clip.w)
        && !outside_clip_plane(&corners, |clip| clip.z < 0.0)
        && !outside_clip_plane(&corners, |clip| clip.z > clip.w)
        && !outside_clip_plane(&corners, |clip| clip.w <= 0.0)
}

fn outside_clip_plane(corners: &[Vec4; 8], outside: impl Fn(&Vec4) -> bool) -> bool {
    corners.iter().all(outside)
}
