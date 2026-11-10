//! Projects server-supplied locator markers inside the pack's custom control.

use super::{paint_sprite, visible_bounds};
use std::sync::Arc;

use json_ui::TextureSource;
use ui_engine::engine::Canvas;

const BACKGROUND: &str = "textures/ui/locator_bg";
const DOT: &str = "textures/ui/locator_bar_dot_0";
const UP: &str = "textures/ui/locator_arrow_up";
const DOWN: &str = "textures/ui/locator_arrow_down";
const HALF_HORIZONTAL_ANGLE: f32 = std::f32::consts::PI / 3.0;

/// The presented camera, with a unit forward vector and vertical FOV in radians.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LocatorView {
    pub position: [f32; 3],
    pub forward: [f32; 3],
    /// Preserves the heading when the camera looks straight up or down.
    pub right: [f32; 3],
    pub vertical_fov: f32,
}

/// One server location and its optional pack-owned icon.
#[derive(Clone, Debug, PartialEq)]
pub struct LocatorMarker {
    pub position: [f32; 3],
    pub color: [u8; 4],
    pub texture: Option<Arc<str>>,
    pub size: Option<[f32; 2]>,
}

/// Locations captured with the view they are projected against.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocatorPaint {
    pub view: LocatorView,
    pub markers: Vec<LocatorMarker>,
}

impl LocatorPaint {
    /// Includes dynamically supplied waypoint paths in texture preparation.
    pub(super) fn textures(&self) -> impl Iterator<Item = &str> {
        [BACKGROUND, DOT, UP, DOWN].into_iter().chain(
            self.markers
                .iter()
                .filter_map(|marker| marker.texture.as_deref()),
        )
    }
}

/// Returns a horizontal bar fraction and the sign of the out-of-view vertical angle.
fn project(view: LocatorView, position: [f32; 3]) -> Option<(f32, i8)> {
    if !view
        .position
        .into_iter()
        .chain(view.forward)
        .chain(view.right)
        .chain(position)
        .all(f32::is_finite)
        || !view.vertical_fov.is_finite()
        || view.vertical_fov <= 0.0
    {
        return None;
    }
    let [dx, dy, dz] =
        std::array::from_fn(|i| f64::from(position[i]) - f64::from(view.position[i]));
    let [fx, fy, fz] = view.forward.map(f64::from);
    let heading = fx.hypot(fz);
    let [hx, hz] = if heading > f64::from(f32::EPSILON) {
        [fx, fz]
    } else {
        [f64::from(view.right[2]), -f64::from(view.right[0])]
    };
    if hx.hypot(hz) <= f64::from(f32::EPSILON) {
        return None;
    }
    let bearing = (-dx * hz + dz * hx).atan2(dx * hx + dz * hz);
    if bearing.abs() > f64::from(HALF_HORIZONTAL_ANGLE) {
        return None;
    }
    let elevation = dy.atan2(dx.hypot(dz)) - fy.atan2(heading);
    let arrow = if elevation > f64::from(view.vertical_fov) * 0.5 {
        1
    } else if elevation < -f64::from(view.vertical_fov) * 0.5 {
        -1
    } else {
        0
    };
    Some((
        (0.5 + bearing / (2.0 * f64::from(HALF_HORIZONTAL_ANGLE))) as f32,
        arrow,
    ))
}

/// Draws the background and markers at the inherited position, scale, alpha and clip.
pub(super) fn paint(
    canvas: &mut Canvas<'_>,
    paint: &LocatorPaint,
    dest: [f32; 4],
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) {
    let visible = visible_bounds(canvas);
    let px = canvas.px();
    if let Some(meta) = canvas.textures().texture(BACKGROUND) {
        let rect = json_ui::Rect::new(
            f64::from(dest[0] / px),
            f64::from(dest[1] / px),
            f64::from((dest[2] - dest[0]) / px),
            f64::from((dest[3] - dest[1]) / px),
        );
        for quad in json_ui::nine_slice(rect, &meta) {
            if let Some(visual) =
                canvas.sprite(BACKGROUND, quad.uv, alpha([255; 4]), Default::default())
            {
                let rect = quad.dest;
                let _ = canvas.push(
                    visual,
                    [
                        rect.x as f32 * px,
                        rect.y as f32 * px,
                        (rect.x + rect.w) as f32 * px,
                        (rect.y + rect.h) as f32 * px,
                    ],
                );
            }
        }
    }
    for marker in &paint.markers {
        let Some((fraction, arrow)) = project(paint.view, marker.position) else {
            continue;
        };
        let texture = marker.texture.as_deref().unwrap_or(DOT);
        let Some(meta) = canvas.textures().texture(texture) else {
            continue;
        };
        let size = marker
            .size
            .unwrap_or_else(|| meta.base_size.map(|side| side as f32));
        if !size.iter().all(|side| side.is_finite() && *side > 0.0) {
            continue;
        }
        let [width, height] = size.map(|side| side * px);
        let center = dest[0] + width * 0.5 + fraction * ((dest[2] - dest[0]) - width).max(0.0);
        let top = (dest[1] + dest[3] - height) * 0.5;
        paint_sprite(
            canvas,
            None,
            texture,
            [
                center - width * 0.5,
                top,
                center + width * 0.5,
                top + height,
            ],
            marker.color,
            visible,
            alpha,
        );
        if arrow != 0 {
            let texture = if arrow > 0 { UP } else { DOWN };
            let Some(meta) = canvas.textures().texture(texture) else {
                continue;
            };
            let [arrow_width, arrow_height] = meta.base_size.map(|side| side as f32 * px);
            let y = if arrow > 0 {
                top - arrow_height
            } else {
                top + height
            };
            paint_sprite(
                canvas,
                None,
                texture,
                [
                    center - arrow_width * 0.5,
                    y,
                    center + arrow_width * 0.5,
                    y + arrow_height,
                ],
                marker.color,
                visible,
                alpha,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A level camera facing south, with a sixty-degree vertical field of view.
    fn view() -> LocatorView {
        LocatorView {
            forward: [0.0, 0.0, 1.0],
            vertical_fov: std::f32::consts::PI / 3.0,
            ..Default::default()
        }
    }

    #[test]
    fn projects_in_front_and_follows_camera_rotation() {
        assert_eq!(project(view(), [0.0, 0.0, 10.0]), Some((0.5, 0)));
        assert!(project(view(), [0.0, 0.0, -10.0]).is_none());
        assert!(project(view(), [-5.0, 0.0, 10.0]).unwrap().0 > 0.5);
        assert_eq!(
            project(
                LocatorView {
                    forward: [1.0, 0.0, 0.0],
                    ..view()
                },
                [10.0, 0.0, 0.0]
            ),
            Some((0.5, 0))
        );
    }

    #[test]
    fn vertical_arrows_follow_the_camera_and_invalid_values_never_project() {
        assert_eq!(project(view(), [0.0, 20.0, 10.0]).unwrap().1, 1);
        assert_eq!(project(view(), [0.0, -20.0, 10.0]).unwrap().1, -1);
        assert!(project(view(), [f32::NAN, 0.0, 0.0]).is_none());
        assert!(project(LocatorView::default(), [0.0, 0.0, 10.0]).is_none());
        assert_eq!(
            project(
                LocatorView {
                    forward: [0.0, 1.0, 0.0],
                    right: [-1.0, 0.0, 0.0],
                    ..view()
                },
                [0.0, 0.0, 10.0]
            ),
            Some((0.5, -1))
        );
    }
}
