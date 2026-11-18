//! Directional focus between and within the painted regions of a screen,
//! such as the play tabs, the Servers tab's list and its details panel.
//!
//! A region whose controls sit side by side is a row: left and right walk it
//! and up and down leave it. Other regions are columns: up and down move to
//! the nearest control below or above, and left and right to a control on the
//! same line, such as a world's Edit button, before leaving the region. At
//! a column's end focus scrolls the region while it can, then moves to the
//! region beyond; it never wraps around.

use {
    super::super::{MenuRuntime, focus::same_control},
    launcher::menu::{
        MenuAction, MenuScreen,
        view::{SettingsFocusAxis, SettingsFocusLandmark, SettingsFocusTarget},
    },
    ui::{UiPoint, UiRect},
};

/// Wheel notches one key press scrolls a region whose controls ran out.
const SCROLL_NOTCHES: f32 = 1.0;
/// Window pixels two edges may overlap and still count as side by side.
const TOUCH: f32 = 0.5;

/// Where one directional press takes focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    /// Focus moves to this control.
    Move(MenuAction),
    /// Focus stays, at an edge or while the region scrolls.
    Hold,
    /// The focused control is outside the region rules; the screen's own order applies.
    Pass,
}

#[derive(Debug)]
struct Region {
    id: u16,
    bounds: UiRect,
    targets: Vec<(MenuAction, UiRect)>,
    /// Whether every control shares one line, so the region reads left to right.
    row: bool,
    /// Whether the region's scroll view can still move up and down.
    scroll: (bool, bool),
}

/// What focus remembers about one region across frames.
#[derive(Debug)]
struct Memory {
    id: u16,
    /// The region's first control when last painted; a new one means it shows
    /// other content, such as another server's details.
    first: MenuAction,
    /// The control last focused there.
    focused: Option<MenuAction>,
}

fn centre_x(b: &UiRect) -> f32 {
    (b.min().x() + b.max().x()) * 0.5
}

fn overlap(a0: f32, a1: f32, b0: f32, b1: f32) -> bool {
    a0 < b1 && b0 < a1
}

/// The space from `from` to `to` along `axis` in `direction`; negative when they overlap.
fn gap(from: &UiRect, to: &UiRect, axis: SettingsFocusAxis, direction: i32) -> f32 {
    let (from_min, from_max, to_min, to_max) = match axis {
        SettingsFocusAxis::Horizontal => {
            (from.min().x(), from.max().x(), to.min().x(), to.max().x())
        }
        SettingsFocusAxis::Vertical => (from.min().y(), from.max().y(), to.min().y(), to.max().y()),
    };
    if direction > 0 {
        to_min - from_max
    } else {
        from_min - to_max
    }
}

/// Whether `a` and `b` share some span across `axis`.
fn aligned(a: &UiRect, b: &UiRect, axis: SettingsFocusAxis) -> bool {
    match axis {
        SettingsFocusAxis::Horizontal => {
            overlap(a.min().y(), a.max().y(), b.min().y(), b.max().y())
        }
        SettingsFocusAxis::Vertical => overlap(a.min().x(), a.max().x(), b.min().x(), b.max().x()),
    }
}

impl Region {
    /// The control a column moves to from target `from`: the nearest one
    /// wholly below or above, lined up with it, for up and down; the nearest
    /// one on its line for left and right.
    fn within(&self, from: usize, axis: SettingsFocusAxis, direction: i32) -> Option<usize> {
        let current = self.targets[from].1;
        self.targets
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != from)
            .filter_map(|(index, (_, b))| {
                let across = (centre_x(b) - centre_x(&current)).abs();
                if axis == SettingsFocusAxis::Vertical {
                    let gap = gap(&current, b, axis, direction);
                    (gap >= -TOUCH).then_some((index, gap, across))
                } else {
                    let ahead = (centre_x(b) - centre_x(&current)) * direction as f32;
                    (aligned(&current, b, axis) && ahead > TOUCH).then_some((index, 0.0, across))
                }
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)))
            .map(|(index, _, _)| index)
    }

    fn centre(&self) -> Option<UiPoint> {
        UiPoint::new(
            centre_x(&self.bounds),
            (self.bounds.min().y() + self.bounds.max().y()) * 0.5,
        )
        .ok()
    }
}

/// The painted regions of the current screen and what focus remembers about each.
#[derive(Debug, Default)]
pub(in crate::menu) struct FocusRegions {
    screen: Option<MenuScreen>,
    regions: Vec<Region>,
    memory: Vec<Memory>,
    scroll: Option<(UiPoint, f32)>,
}

impl FocusRegions {
    /// Replaces the painted regions of `screen`, keeping what each region
    /// remembers while the screen stays the same. `extent` reports the scroll
    /// view under a point as its heading offset and largest offset.
    pub(super) fn update(
        &mut self,
        screen: MenuScreen,
        targets: &[SettingsFocusTarget],
        landmarks: &[SettingsFocusLandmark],
        extent: impl Fn(UiPoint) -> Option<(f32, f32)>,
    ) {
        if self.screen != Some(screen) {
            self.memory.clear();
        }
        self.screen = Some(screen);
        self.regions.clear();
        for landmark in landmarks {
            let mut targets: Vec<_> = targets
                .iter()
                .filter(|target| target.landmark == Some(landmark.id))
                .map(|target| (target.action, target.bounds))
                .collect();
            let Some(&(_, first_bounds)) = targets.first() else {
                continue;
            };
            let row = targets.len() > 1
                && targets.iter().all(|(_, bounds)| {
                    aligned(&first_bounds, bounds, SettingsFocusAxis::Horizontal)
                });
            if row {
                // Painters may draw a raised control last; a row reads by position.
                targets.sort_by(|a, b| a.1.min().x().total_cmp(&b.1.min().x()));
            }
            let mut region = Region {
                id: landmark.id,
                bounds: landmark.bounds,
                targets,
                row,
                scroll: (false, false),
            };
            if let Some((offset, max)) = region.centre().and_then(&extent) {
                region.scroll = (offset > TOUCH, offset < max - TOUCH);
            }
            let first = region.targets[0].0;
            match self.memory.iter_mut().find(|memory| memory.id == region.id) {
                // Other content: entering it starts at its first control again.
                Some(memory) if !same_control(memory.first, first) => {
                    memory.first = first;
                    memory.focused = None;
                }
                Some(_) => {}
                None => self.memory.push(Memory {
                    id: region.id,
                    first,
                    focused: None,
                }),
            }
            self.regions.push(region);
        }
    }

    /// Forgets the regions, for screens that do not move focus between them.
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    /// The scroll a key press asked for, as a point in the region and wheel notches.
    pub(in crate::menu) fn take_scroll(&mut self) -> Option<(UiPoint, f32)> {
        self.scroll.take()
    }

    fn locate(&self, action: MenuAction) -> Option<(usize, usize)> {
        self.regions.iter().enumerate().find_map(|(r, region)| {
            region
                .targets
                .iter()
                .position(|(candidate, _)| same_control(*candidate, action))
                .map(|t| (r, t))
        })
    }

    /// The nearest region beside `from` along `axis` in `direction`, preferring
    /// one that lines up with `near` across that axis.
    fn neighbour(
        &self,
        from: usize,
        axis: SettingsFocusAxis,
        direction: i32,
        near: UiRect,
    ) -> Option<usize> {
        let origin = self.regions[from].bounds;
        self.regions
            .iter()
            .enumerate()
            .filter(|(r, _)| *r != from)
            .filter_map(|(r, region)| {
                let gap = gap(&origin, &region.bounds, axis, direction);
                let apart = !aligned(&near, &region.bounds, axis);
                (gap >= -TOUCH).then_some((r, apart, gap))
            })
            .min_by(|a, b| a.1.cmp(&b.1).then(a.2.total_cmp(&b.2)))
            .map(|(r, _, _)| r)
    }

    /// The control focus enters `region` on: the one last focused there while
    /// it is still painted, otherwise the one nearest `near` in a row, or the first.
    fn entry(&self, region: usize, near: UiRect) -> MenuAction {
        let region = &self.regions[region];
        let remembered = self
            .memory
            .iter()
            .find(|memory| memory.id == region.id)
            .and_then(|memory| memory.focused)
            .filter(|action| {
                region
                    .targets
                    .iter()
                    .any(|(candidate, _)| same_control(*candidate, *action))
            });
        if let Some(action) = remembered {
            return action;
        }
        let centre = centre_x(&near);
        let distance = |bounds: &UiRect| (centre_x(bounds) - centre).abs();
        region
            .targets
            .iter()
            .filter(|_| region.row)
            .min_by(|a, b| distance(&a.1).total_cmp(&distance(&b.1)))
            .map_or(region.targets[0].0, |(action, _)| *action)
    }

    fn remember(&mut self, region: usize, action: MenuAction) {
        let id = self.regions[region].id;
        if let Some(memory) = self.memory.iter_mut().find(|memory| memory.id == id) {
            memory.focused = Some(action);
        }
    }

    /// Where a directional press from `current` takes focus.
    fn step(&mut self, current: MenuAction, axis: SettingsFocusAxis, direction: i32) -> Step {
        let Some((r, t)) = self.locate(current) else {
            return Step::Pass;
        };
        let region = &self.regions[r];
        let near = region.targets[t].1;
        let column = !region.row && axis == SettingsFocusAxis::Vertical;
        if !region.row || axis == SettingsFocusAxis::Horizontal {
            let next = if region.row {
                usize::try_from(t as i32 + direction).ok()
            } else {
                region.within(t, axis, direction)
            };
            if let Some((action, _)) = next.and_then(|next| region.targets.get(next)) {
                return Step::Move(*action);
            }
            if column {
                let (up, down) = region.scroll;
                if (direction > 0 && down) || (direction < 0 && up) {
                    self.scroll = region
                        .centre()
                        .map(|centre| (centre, -SCROLL_NOTCHES * direction as f32));
                    return Step::Hold;
                }
            } else if region.row {
                return Step::Hold;
            }
        }
        match self.neighbour(r, axis, direction, near) {
            Some(next) => {
                let action = self.entry(next, near);
                self.remember(r, current);
                self.remember(next, action);
                Step::Move(action)
            }
            // Focus holds at a column's ends and sides and at the top of the
            // screen; below a row, controls outside every region take over.
            None if column || axis == SettingsFocusAxis::Horizontal || direction < 0 => Step::Hold,
            None => Step::Pass,
        }
    }
}

impl MenuRuntime {
    /// Moves focus by one directional press under the region rules. Returns
    /// `false` when the focused control is outside every region, so the caller
    /// keeps its own order.
    pub(in crate::menu) fn move_in_regions(
        &mut self,
        axis: SettingsFocusAxis,
        direction: i32,
    ) -> bool {
        if self.navigation_focus.regions.screen != Some(self.screen) {
            return false;
        }
        let actions = self.focus_actions();
        let Some(current) = actions.get(self.focused).copied() else {
            return false;
        };
        let next = match self.navigation_focus.regions.step(current, axis, direction) {
            Step::Pass => return false,
            Step::Hold => return true,
            Step::Move(next) => next,
        };
        if let Some(index) = actions
            .iter()
            .position(|action| same_control(*action, next))
        {
            self.focused = index;
            self.field = None;
            self.navigation_focus.record_input(Some(next));
        }
        true
    }
}

#[cfg(test)]
mod tests;
