//! Carried bamboo gates reuse each piece's artwork with their carried face directions.

use super::*;
use assets::gui_item::GuiBlockQuad;

/// Places existing cap, bar and rail regions without changing carried geometry.
pub(super) fn model(
    world: &RuntimeAssets,
    visual: u32,
) -> Option<([u32; 6], Vec<crate::ActorRigVertex>)> {
    let mut quads = gates::carried_quads(world, visual)?;
    let (parts, remainder) = quads.as_chunks_mut::<6>();
    if !remainder.is_empty() {
        return None;
    }
    let bounds: Vec<_> = parts.iter().map(piece_bounds).collect();
    let (rail_height, post_height) = bounds.iter().fold(
        (f32::INFINITY, f32::NEG_INFINITY),
        |(lo, hi), (min, max)| {
            let height = max.y - min.y;
            (lo.min(height), hi.max(height))
        },
    );
    for (part, (min, max)) in parts.iter_mut().zip(bounds) {
        let original = *part;
        let height = max.y - min.y;
        let near_post = height == post_height && max.z < 0.5;
        let rail = height == rail_height;
        for quad in part {
            let direction = face_normal(quad);
            let source = if direction.z < 0.0 {
                original.iter().find(|q| face_normal(q).x > 0.0)?
            } else if direction.z > 0.0 {
                original.iter().find(|q| face_normal(q).x < 0.0)?
            } else {
                &*quad
            };
            let (uv_min, uv_max) = source.uvs.iter().map(|&uv| glam::Vec2::from(uv)).fold(
                (
                    glam::Vec2::splat(f32::INFINITY),
                    glam::Vec2::splat(f32::NEG_INFINITY),
                ),
                |(lo, hi), uv| (lo.min(uv), hi.max(uv)),
            );
            quad.uvs = quad.corners.map(|p| {
                let [x, y, z] = ((glam::Vec3::from(p) - min) / (max - min)).to_array();
                let unit = if direction.y != 0.0 {
                    if near_post {
                        [x, z]
                    } else {
                        [1.0 - z, if rail { 1.0 - x } else { x }]
                    }
                } else if direction.z < 0.0 {
                    [1.0 - x, 1.0 - y]
                } else if direction.z > 0.0 {
                    [x, 1.0 - y]
                } else if direction.x < 0.0 {
                    [z, 1.0 - y]
                } else {
                    [1.0 - z, 1.0 - y]
                };
                (uv_min + glam::Vec2::from(unit) * (uv_max - uv_min)).to_array()
            });
        }
    }
    template_quads(&quads)
}

/// Measures a cuboid from all its outward-facing surfaces.
fn piece_bounds(part: &[GuiBlockQuad; 6]) -> (glam::Vec3, glam::Vec3) {
    part.iter()
        .flat_map(|q| q.corners)
        .map(glam::Vec3::from)
        .fold(
            (
                glam::Vec3::splat(f32::INFINITY),
                glam::Vec3::splat(f32::NEG_INFINITY),
            ),
            |(lo, hi), p| (lo.min(p), hi.max(p)),
        )
}

/// Returns the outward direction of an authored face.
fn face_normal(quad: &GuiBlockQuad) -> glam::Vec3 {
    let [a, b, c, _] = quad.corners.map(glam::Vec3::from);
    (b - a).cross(c - a)
}
