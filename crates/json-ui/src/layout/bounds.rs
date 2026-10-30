//! Paint overflow used by both indexed placements and per-node scroll culling.

use super::{LayoutEnv, Rect, ResolvedControl, measure};
use crate::transform::Affine;
use serde_json::Value;

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    /// The overlap of two rects, clamped so width/height never go negative.
    pub fn intersect(self, other: Rect) -> Rect {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.w).min(other.x + other.w);
        let y1 = (self.y + self.h).min(other.y + other.h);
        Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
    }

    /// The smallest rectangle containing both rectangles.
    pub(crate) fn union(self, other: Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect::new(
            x,
            y,
            (self.x + self.w).max(other.x + other.w) - x,
            (self.y + self.h).max(other.y + other.h) - y,
        )
    }

    /// Extends every edge by a nonnegative coverage allowance.
    pub(crate) fn expanded(self, padding: f64) -> Rect {
        Rect::new(
            self.x - padding,
            self.y - padding,
            self.w + 2.0 * padding,
            self.h + 2.0 * padding,
        )
    }
}

/// Child paint in its parent's layout coordinates, retaining authored input and child bounds.
pub(super) fn subtree(
    control: &ResolvedControl,
    rect: Rect,
    env: &LayoutEnv,
    parent: Option<Affine>,
) -> Rect {
    let own = measure::style(control)
        .transform
        .filter(|transform| !transform.is_identity())
        .map(|transform| transform.about(rect));
    let transform = match (parent, own) {
        (Some(parent), Some(own)) => Some(parent.then(&own)),
        (parent, own) => parent.or(own),
    };
    let paint = if control.control_type.as_deref() == Some("custom")
        && matches!(
            control.properties.get("renderer").and_then(Value::as_str),
            Some(crate::rounded::RENDERER | crate::vector_path::RENDERER)
        ) {
        match crate::emit::custom_draw(control) {
            Some(crate::Draw::Custom { renderer, data })
                if renderer == crate::rounded::RENDERER =>
            {
                crate::rounded::Shadow::read(&data).map_or(rect, |shadow| {
                    shadow.subtree_bounds(rect, env.text.physical_pixel_size(), transform)
                })
            }
            Some(crate::Draw::Custom { data, .. }) => crate::vector_path::bounds(&data)
                .map_or(rect, |bounds| {
                    bounds.subtree_at(rect, env.text.physical_pixel_size())
                }),
            _ => rect,
        }
    } else {
        rect
    };
    rect.union(own.map_or(paint, |own| own.bounds(paint)))
}

/// Returns the screen clip in a parent's layout coordinates; a collapsed parent cannot paint.
pub(super) fn local_clip(parent: Option<Affine>, clip: Rect) -> Option<Rect> {
    match parent {
        Some(parent) => parent.inverse().map(|inverse| inverse.bounds(clip)),
        None => Some(clip),
    }
}
