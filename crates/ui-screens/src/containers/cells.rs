//! The cells a drawn container screen lets a pointer press, found by inverting
//! [`ScreenLayout::hit`] over the frame's regions.

use json_ui::{HitKind, HitRegion, hit_test};
use ui_engine::frame::EngineFrame;

use super::ScreenLayout;
use crate::container_actions::InventoryCellHit;

impl ScreenLayout {
    /// Every inventory cell this layout drew in `frame`, once each, with a point where a
    /// pointer press reaches it (virtual px equal GUI px). It inverts [`Self::hit`] over the
    /// item-cell regions, so a caller that names a cell takes the same path as a pointer press.
    ///
    /// For each cell region, only the controls drawn above it can take a press away from it.
    /// A sweep over their boxes finds the exposed point nearest the region's centre, so a cell
    /// shown through even a thin strip is found, in time bounded by the controls that overlap
    /// it rather than by the whole screen.
    pub fn cells(self, frame: &EngineFrame) -> Vec<(InventoryCellHit, [f32; 2])> {
        let regions = &frame.hits;
        let boxes: Vec<Option<[f64; 4]>> = regions.iter().map(drawn_box).collect();
        let mut cells: Vec<(InventoryCellHit, [f32; 2])> = Vec::new();
        for (index, region) in regions.iter().enumerate() {
            if !region.enabled || region.collection.is_none() || region.collection_index.is_none() {
                continue;
            }
            let Some(bounds) = boxes[index] else {
                continue;
            };
            // `press_region` takes the last region in draw order that is not a bare panel.
            let covers: Vec<[f64; 4]> = regions
                .iter()
                .zip(&boxes)
                .skip(index + 1)
                .filter(|(other, _)| other.kind != HitKind::Panel || other.pressed.is_some())
                .filter_map(|(_, other)| *other)
                .filter(|other| overlaps(*other, bounds))
                .collect();
            let reached = exposed_points(bounds, &covers)
                .into_iter()
                .find_map(|point| {
                    #[allow(clippy::cast_possible_truncation, reason = "GUI coordinates are small")]
                    let gui = [point[0] as f32, point[1] as f32];
                    let top = hit_test(regions, [f64::from(gui[0]), f64::from(gui[1])])?;
                    std::ptr::eq(top, region).then_some(gui)
                });
            if let Some(point) = reached
                && let Some(hit) = self.hit(frame, point)
                && !cells.iter().any(|(known, _)| *known == hit)
            {
                cells.push((hit, point));
            }
        }
        cells
    }
}

/// Points of `bounds` that none of `covers` contains, nearest the centre first: for each
/// vertical strip between cover edges, the free point of each vertical gap nearest the centre.
/// Every exposed part of `bounds` contains one of them.
fn exposed_points(bounds: [f64; 4], covers: &[[f64; 4]]) -> Vec<[f64; 2]> {
    let [left, top, right, bottom] = bounds;
    let centre = [(left + right) / 2.0, (top + bottom) / 2.0];
    let mut edges = vec![left, right];
    edges.extend(
        covers
            .iter()
            .flat_map(|cover| [cover[0], cover[2]])
            .filter(|edge| *edge > left && *edge < right),
    );
    edges.sort_by(f64::total_cmp);
    edges.dedup();
    let mut points = Vec::new();
    for strip in edges.windows(2) {
        let x = if (strip[0]..strip[1]).contains(&centre[0]) {
            centre[0]
        } else {
            (strip[0] + strip[1]) / 2.0
        };
        let mut spans: Vec<[f64; 2]> = covers
            .iter()
            .filter(|cover| cover[0] <= x && x < cover[2])
            .map(|cover| [cover[1].max(top), cover[3].min(bottom)])
            .filter(|span| span[1] > span[0])
            .collect();
        spans.sort_by(|a, b| a[0].total_cmp(&b[0]));
        let mut free = top;
        let mut gaps = Vec::new();
        for span in spans {
            if span[0] > free {
                gaps.push([free, span[0]]);
            }
            free = free.max(span[1]);
        }
        if bottom > free {
            gaps.push([free, bottom]);
        }
        points.extend(gaps.into_iter().map(|[low, high]| {
            let y = if (low..high).contains(&centre[1]) {
                centre[1]
            } else {
                (low + high) / 2.0
            };
            [x, y]
        }));
    }
    let distance =
        |point: &[f64; 2]| (point[0] - centre[0]).powi(2) + (point[1] - centre[1]).powi(2);
    points.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
    points
}

/// Whether two `[left, top, right, bottom]` boxes share any area.
fn overlaps(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// Where a region is drawn and can be pressed, as `[left, top, right, bottom]`: its rect
/// (through its paint transform's bounding box) within its clip; `None` when empty.
fn drawn_box(region: &HitRegion) -> Option<[f64; 4]> {
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
