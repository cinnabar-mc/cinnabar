//! The cells a drawn container screen lets a pointer press, found by inverting
//! [`ScreenLayout::hit`] over the frame's regions.

use ui_engine::frame::EngineFrame;

use super::ScreenLayout;
use crate::container_actions::InventoryCellHit;

impl ScreenLayout {
    /// Every ledger cell or control this layout drew in `frame`, once each, with a point
    /// where a press reaches it (virtual px equal GUI px). It inverts [`Self::hit`], so a
    /// caller that names a cell takes the same path as a pointer press on it.
    ///
    /// Each region's drawn box is split at the box edges of every region, so each piece is
    /// either wholly covered or wholly exposed by any axis-aligned control; testing one
    /// point per piece finds every cell a pointer can reach, however little of it is
    /// exposed, at the reachable point nearest its region's centre.
    pub fn cells(self, frame: &EngineFrame) -> Vec<(InventoryCellHit, [f32; 2])> {
        let boxes: Vec<[f64; 4]> = frame.hits.iter().filter_map(drawn_box).collect();
        let mut cells: Vec<(InventoryCellHit, [f32; 2])> = Vec::new();
        for &[left, top, right, bottom] in &boxes {
            let cuts = |low: f64, high: f64, axis: usize| {
                let mut edges = vec![low, high];
                for other in &boxes {
                    for edge in [other[axis], other[axis + 2]] {
                        if edge > low && edge < high {
                            edges.push(edge);
                        }
                    }
                }
                edges.sort_by(f64::total_cmp);
                edges.dedup();
                edges
                    .windows(2)
                    .map(|pair| (pair[0] + pair[1]) / 2.0)
                    .collect::<Vec<_>>()
            };
            let centre = [(left + right) / 2.0, (top + bottom) / 2.0];
            let xs = cuts(left, right, 0);
            let ys = cuts(top, bottom, 1);
            let mut points: Vec<[f64; 2]> = ys
                .iter()
                .flat_map(|y| xs.iter().map(move |x| [*x, *y]))
                .collect();
            points.sort_by(|a, b| {
                let distance =
                    |p: &[f64; 2]| (p[0] - centre[0]).powi(2) + (p[1] - centre[1]).powi(2);
                distance(a).total_cmp(&distance(b))
            });
            for point in points {
                #[allow(clippy::cast_possible_truncation, reason = "GUI coordinates are small")]
                let point = [point[0] as f32, point[1] as f32];
                if let Some(hit) = self.hit(frame, point)
                    && !cells.iter().any(|(known, _)| *known == hit)
                {
                    cells.push((hit, point));
                }
            }
        }
        cells
    }
}

/// Where a region is drawn and can be pressed, as `[left, top, right, bottom]`: its rect
/// (through its paint transform's bounding box) within its clip; `None` when empty.
fn drawn_box(region: &json_ui::HitRegion) -> Option<[f64; 4]> {
    let rect = &region.rect;
    let corners = [
        [rect.x, rect.y],
        [rect.x + rect.w, rect.y],
        [rect.x, rect.y + rect.h],
        [rect.x + rect.w, rect.y + rect.h],
    ]
    .map(|corner| {
        region
            .transform
            .map_or(corner, |transform| transform.apply(corner))
    });
    let fold = |pick: fn(f64, f64) -> f64, axis: usize| {
        corners
            .iter()
            .map(|corner| corner[axis])
            .reduce(pick)
            .unwrap_or_default()
    };
    let clip = &region.clip;
    let left = fold(f64::min, 0).max(clip.x);
    let top = fold(f64::min, 1).max(clip.y);
    let right = fold(f64::max, 0).min(clip.x + clip.w);
    let bottom = fold(f64::max, 1).min(clip.y + clip.h);
    (right > left && bottom > top).then_some([left, top, right, bottom])
}
