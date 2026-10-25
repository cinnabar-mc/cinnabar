//! Original, Bevy-free controller navigation foundation; no item operations.
//!
//! Wiring: associate each current engine hit key with a `Target`, call `update`
//! once per input sample, then use `selection` only after action-time authority
//! validation. Copy `cursor`/`focused_key` into presentation; `reveal_key` is a
//! request for the coordinator's scroll owner, not a scroll mutation.
//!
//! Sample order: neutral sample on open, Right held (immediate focus), subsequent
//! caller times (repeat), neutral (release), axes (cursor). Call `reset` on focus
//! loss, disconnect, modal interruption or close. Screen/controller replacement
//! also resets automatically. No held input is replayed across those boundaries.
//!
//! Pack evidence: pinned v1.26.50.4 `ui_common.json` slot prototype authors focus,
//! overrides, no-wrap and magnet metadata. Navigation delegates to JSON-UI.
//! Cursor curve, diagonal arbitration and repeat are provisional Cinnabar policy,
//! NOT measured Bedrock behavior. No physical button/stick roles are assigned.
//! Magnet snapping and virtualized neighbor materialization remain incomplete.

use std::{collections::BTreeMap, time::Duration};

use json_ui::{FocusDirection, FocusMove, HitRegion, RectOut, ViewState};
use ui_screens::container_actions::InventoryCellHit;

/// Association supplied by the screen owner; addresses are not identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub key: String,
    /// Stable within a screen generation, including across layout/key changes.
    /// For filtered recipes/catalogs this must identify content, NOT row index.
    pub identity: u64,
    pub hit: InventoryCellHit,
}

/// Current geometry and semantic associations, all in GUI coordinates.
/// Keys and identities in `targets` must each be unique. Retain non-slot hits:
/// they carry focus mappings, containers, modal barriers and other controls.
pub struct Frame<'a> {
    pub generation: u64,
    pub geometry_revision: u64,
    pub bounds: RectOut,
    pub hits: &'a [HitRegion],
    pub targets: &'a [Target],
}

/// Explicit provisional UI settings; deliberately no camera-derived/default values.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub cursor_units_per_second: f64,
    /// Radial deadzone in [0, 1); remaining magnitude is linearly rescaled.
    pub cursor_deadzone: f64,
    pub repeat_delay: Duration,
    pub repeat_interval: Duration,
}

impl Settings {
    /// Reject invalid configuration rather than allowing NaN motion or busy repeats.
    pub fn validate(self) -> Result<Self, &'static str> {
        if !self.cursor_units_per_second.is_finite() || self.cursor_units_per_second < 0.0 {
            return Err("UI cursor speed must be finite and nonnegative");
        }
        if !self.cursor_deadzone.is_finite() || !(0.0..1.0).contains(&self.cursor_deadzone) {
            return Err("UI cursor deadzone must be in [0, 1)");
        }
        if self.repeat_interval.is_zero() {
            return Err("UI repeat interval must be positive");
        }
        Ok(self)
    }
}

/// One normalized input sample. Axes use +x right/+y down; nonfinite axes are zero.
pub struct Input {
    pub now: Duration,
    pub controller: u64,
    pub active: bool,
    /// Held Up, Down, Left, Right. Opposites cancel; vertical wins diagonals
    /// (provisional policy). Direction takes priority over simultaneous axes.
    pub directions: [bool; 4],
    pub axes: [f64; 2],
    /// Only genuine meaningful mouse movement/clicks, never a stale OS position.
    /// Takes precedence over controller input in this sample.
    pub mouse: Option<[f64; 2]>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Inactive,
    Focus,
    Cursor,
    Mouse,
}

/// Selection token for downstream revalidation, never permission to mutate items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub generation: u64,
    pub geometry_revision: u64,
    pub target: Target,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    pub mode: Mode,
    pub cursor: Option<[f64; 2]>,
    pub focused_key: Option<String>,
    pub selection: Option<Selection>,
    /// Only emitted on focus acquisition/movement, not every frame.
    pub reveal_key: Option<String>,
    /// JSON-UI control claimed the direction; coordinator may route its event.
    /// Coalesced to one notification per sample, not replayed across elapsed time.
    pub claimed_direction: Option<(String, FocusDirection)>,
}

/// Retains only local navigation state. Caller owns screen/ledger authority.
pub struct Navigation {
    settings: Settings,
    owner: Option<(u64, u64)>,
    previous_time: Option<Duration>,
    armed: bool,
    held: Option<FocusDirection>,
    next_repeat: Option<Duration>,
    view: ViewState,
    output: Output,
}

impl Navigation {
    pub fn new(settings: Settings) -> Result<Self, &'static str> {
        Ok(Self {
            settings: settings.validate()?,
            owner: None,
            previous_time: None,
            armed: false,
            held: None,
            next_repeat: None,
            view: ViewState::default(),
            output: Output::default(),
        })
    }

    /// Cancel focus, cursor, repeat and ownership; require neutral before rearming.
    pub fn reset(&mut self) {
        self.owner = None;
        self.previous_time = None;
        self.armed = false;
        self.held = None;
        self.next_repeat = None;
        self.view = ViewState::default();
        self.output = Output::default();
    }

    /// Acquires an existing pointer for action-only controller use, without moving
    /// it or changing its selection. Subsequent neutral samples retain ownership.
    pub fn acquire_cursor(&mut self) -> &Output {
        if self.output.mode == Mode::Mouse && self.output.cursor.is_some() {
            self.output.mode = Mode::Cursor;
        }
        &self.output
    }

    /// Reduce one sample using caller time. Backwards time cancels/rearms safely.
    /// Repeat catches up elapsed deadlines, so focus results do not depend on FPS.
    pub fn update(&mut self, frame: Frame<'_>, input: Input) -> &Output {
        if !input.active || !valid_rect(frame.bounds) {
            self.reset();
            return &self.output;
        }
        let owner = (frame.generation, input.controller);
        if self.owner != Some(owner)
            || self
                .previous_time
                .is_some_and(|previous| input.now < previous)
        {
            self.reset();
            self.owner = Some(owner);
        }
        let elapsed = self
            .previous_time
            .map_or(Duration::ZERO, |previous| input.now - previous);
        self.previous_time = Some(input.now);
        self.output.reveal_key = None;
        self.output.claimed_direction = None;

        // Keep generic routing metadata, but remove nonvisible focus candidates.
        let hits: Vec<_> = frame
            .hits
            .iter()
            .cloned()
            .map(|mut hit| {
                if visible_point(&hit).is_none()
                    && let Some(focus) = &mut hit.focus
                {
                    focus.enabled = false;
                }
                hit
            })
            .collect();
        self.revalidate(&frame, &hits);
        if let Some(mouse) = input
            .mouse
            .filter(|point| point.iter().all(|v| v.is_finite()))
        {
            self.armed = false;
            self.held = None;
            self.next_repeat = None;
            self.output.mode = Mode::Mouse;
            self.output.cursor = Some(clamp(mouse, frame.bounds));
            self.point_at(&frame, &hits);
            return &self.output;
        }
        let velocity = velocity(input.axes, self.settings.cursor_deadzone);
        let neutral = !input.directions.iter().any(|held| *held) && velocity == [0.0; 2];
        if !self.armed {
            self.armed = neutral;
            return &self.output;
        }
        let direction = direction(input.directions);
        if let Some(direction) = direction {
            self.output.mode = Mode::Focus;
            self.focus(&frame, &hits, self.view.focused.clone());
            let mut steps = 0;
            if self.held != Some(direction) {
                steps = 1;
                self.next_repeat = input.now.checked_add(self.settings.repeat_delay);
            } else if let Some(deadline) =
                self.next_repeat.filter(|deadline| input.now >= *deadline)
            {
                let interval = self.settings.repeat_interval.as_nanos();
                steps = (input.now - deadline).as_nanos() / interval + 1;
                let remainder = (input.now - deadline).as_nanos() % interval;
                // Advance from the deadline, not the render sample's phase.
                let until_next = self.settings.repeat_interval - duration_from_nanos(remainder);
                self.next_repeat = input.now.checked_add(until_next);
            }
            self.held = Some(direction);
            let mut visited = BTreeMap::new();
            while steps > 0 {
                // A fixed frame/direction is deterministic over this complete
                // navigation state. Skip cycles rather than imposing a timing cap.
                let state = (
                    self.view.focused.clone(),
                    self.view.focus_memory.held.clone(),
                    self.view.focus_memory.last.clone(),
                );
                if let Some(previous_steps) = visited.insert(state, steps) {
                    let cycle_length = previous_steps - steps;
                    steps %= cycle_length;
                    if steps == 0 {
                        break;
                    }
                    // Remaining work is less than one cycle; old remaining-step
                    // counters are no longer comparable after the skip.
                    visited.clear();
                }
                steps -= 1;
                match json_ui::navigate(&hits, &self.view, direction, frame.bounds) {
                    FocusMove::Moved(key) => {
                        let changed = self.view.focused.as_ref() != Some(&key);
                        self.focus(&frame, &hits, Some(key.clone()));
                        self.output.reveal_key = Some(key);
                        if !changed {
                            break;
                        }
                    }
                    FocusMove::Claimed(key) => {
                        self.output.claimed_direction = Some((key, direction));
                        break;
                    }
                    FocusMove::Stayed => break,
                }
            }
            return &self.output;
        }
        self.held = None;
        self.next_repeat = None;
        if velocity != [0.0; 2] {
            self.output.mode = Mode::Cursor;
            let origin = self.output.cursor.unwrap_or(center(frame.bounds));
            let movement = velocity
                .map(|axis| axis * self.settings.cursor_units_per_second * elapsed.as_secs_f64());
            self.output.cursor = Some(clamp(
                [origin[0] + movement[0], origin[1] + movement[1]],
                frame.bounds,
            ));
            self.point_at(&frame, &hits);
        }
        &self.output
    }

    /// Preserve semantic identity across reflow, never a recycled key/index.
    fn revalidate(&mut self, frame: &Frame<'_>, hits: &[HitRegion]) {
        let admitted = json_ui::focus_order(hits);
        let key = if let Some(selection) = &self.output.selection {
            frame
                .targets
                .iter()
                .find(|target| target.identity == selection.target.identity)
                .map(|target| target.key.clone())
        } else {
            self.view.focused.clone()
        };
        let key = key
            .filter(|key| admitted.iter().any(|hit| hit.key == *key))
            .filter(|key| {
                if !matches!(self.output.mode, Mode::Cursor | Mode::Mouse) {
                    return true;
                }
                self.output
                    .cursor
                    .and_then(|point| json_ui::hit_test(hits, point))
                    .is_some_and(|hit| hit.key == *key)
            });
        if key.is_none() {
            if self.view.focused.is_some() {
                self.armed = false;
                self.held = None;
                self.next_repeat = None;
            }
            self.view.focus_memory = Default::default();
        }
        self.focus(frame, hits, key);
    }

    /// Focus updates cursor only in directional mode; pointer modes remain continuous.
    fn focus(&mut self, frame: &Frame<'_>, hits: &[HitRegion], key: Option<String>) {
        json_ui::set_focus(&mut self.view, hits, key.clone());
        self.view.hovered = key.clone();
        self.output.focused_key = key.clone();
        self.output.selection = key
            .as_ref()
            .and_then(|key| frame.targets.iter().find(|target| target.key == *key))
            .map(|target| Selection {
                generation: frame.generation,
                geometry_revision: frame.geometry_revision,
                target: target.clone(),
            });
        if self.output.mode == Mode::Focus {
            self.output.cursor = key
                .as_ref()
                .and_then(|key| hits.iter().find(|hit| hit.key == *key))
                .and_then(visible_point);
        }
    }

    /// Pointer and directional input resolve through the same admitted target set.
    fn point_at(&mut self, frame: &Frame<'_>, hits: &[HitRegion]) {
        let admitted = json_ui::focus_order(hits);
        let key = self
            .output
            .cursor
            .and_then(|point| json_ui::hit_test(hits, point))
            .filter(|hit| admitted.iter().any(|candidate| candidate.key == hit.key))
            .map(|hit| hit.key.clone());
        self.focus(frame, hits, key);
    }
}

/// Return an interior anchor of the painted rectangle clipped to its viewport.
fn visible_point(hit: &HitRegion) -> Option<[f64; 2]> {
    if !valid_rect(hit.rect) || !valid_rect(hit.clip) {
        return None;
    }
    let rect = hit.rect;
    let points = [
        [rect.x, rect.y],
        [rect.x + rect.w, rect.y],
        [rect.x + rect.w, rect.y + rect.h],
        [rect.x, rect.y + rect.h],
    ]
    .map(|point| {
        hit.transform
            .map_or(point, |transform| transform.apply(point))
    });
    if points.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }
    let mut polygon = points.to_vec();
    for (axis, boundary, keep_greater) in [
        (0, hit.clip.x, true),
        (0, hit.clip.x + hit.clip.w, false),
        (1, hit.clip.y, true),
        (1, hit.clip.y + hit.clip.h, false),
    ] {
        polygon = clip_polygon(polygon, axis, boundary, keep_greater);
    }
    let origin = *polygon.first()?;
    let has_area = polygon[1..].windows(2).any(|edge| {
        let first = [edge[0][0] - origin[0], edge[0][1] - origin[1]];
        let second = [edge[1][0] - origin[0], edge[1][1] - origin[1]];
        let area = first[0] * second[1] - first[1] * second[0];
        area.is_finite() && area != 0.0
    });
    if !has_area {
        return None;
    }
    // A positive-weight mean of every vertex is strictly inside a convex polygon
    // with nonzero area, unlike the midpoint of its axis-aligned bounding box.
    let weight = 1.0 / polygon.len() as f64;
    let point = polygon.iter().fold([0.0; 2], |sum, point| {
        [sum[0] + point[0] * weight, sum[1] + point[1] * weight]
    });
    hit.contains(point).then_some(point)
}

/// Clip a perimeter-ordered convex polygon against one axis-aligned half-plane.
fn clip_polygon(
    polygon: Vec<[f64; 2]>,
    axis: usize,
    boundary: f64,
    keep_greater: bool,
) -> Vec<[f64; 2]> {
    let Some(mut previous) = polygon.last().copied() else {
        return polygon;
    };
    let inside = |point: [f64; 2]| {
        if keep_greater {
            point[axis] >= boundary
        } else {
            point[axis] <= boundary
        }
    };
    let mut clipped = Vec::new();
    for current in polygon {
        if inside(previous) != inside(current) {
            let fraction = (boundary - previous[axis]) / (current[axis] - previous[axis]);
            let mut intersection = [
                previous[0] + (current[0] - previous[0]) * fraction,
                previous[1] + (current[1] - previous[1]) * fraction,
            ];
            intersection[axis] = boundary;
            clipped.push(intersection);
        }
        if inside(current) {
            clipped.push(current);
        }
        previous = current;
    }
    clipped
}

/// Admit only finite, positive GUI rectangles with finite endpoints.
fn valid_rect(rect: RectOut) -> bool {
    [
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        rect.x + rect.w,
        rect.y + rect.h,
    ]
    .iter()
    .all(|value| value.is_finite())
        && rect.w > 0.0
        && rect.h > 0.0
}

/// Midpoint of a validated GUI rectangle.
fn center(rect: RectOut) -> [f64; 2] {
    [rect.x + rect.w / 2.0, rect.y + rect.h / 2.0]
}

/// Keep a continuous cursor within viewport bounds, including its outer edge.
fn clamp(point: [f64; 2], bounds: RectOut) -> [f64; 2] {
    [
        point[0].clamp(bounds.x, bounds.x + bounds.w),
        point[1].clamp(bounds.y, bounds.y + bounds.h),
    ]
}

/// Sanitize axes and apply a radial linear curve without diagonal acceleration.
fn velocity(axes: [f64; 2], deadzone: f64) -> [f64; 2] {
    let axes = axes.map(|axis| {
        if axis.is_finite() {
            axis.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    });
    let length = axes[0].hypot(axes[1]);
    if length <= deadzone {
        return [0.0; 2];
    }
    let magnitude = (length.min(1.0) - deadzone) / (1.0 - deadzone);
    axes.map(|axis| axis / length * magnitude)
}

/// Cancel each opposite pair before applying provisional vertical precedence.
fn direction(held: [bool; 4]) -> Option<FocusDirection> {
    match (
        i8::from(held[1]) - i8::from(held[0]),
        i8::from(held[3]) - i8::from(held[2]),
    ) {
        (-1, _) => Some(FocusDirection::Up),
        (1, _) => Some(FocusDirection::Down),
        (_, -1) => Some(FocusDirection::Left),
        (_, 1) => Some(FocusDirection::Right),
        _ => None,
    }
}

/// Convert a remainder known to be smaller than a representable Duration.
fn duration_from_nanos(nanos: u128) -> Duration {
    Duration::new(
        (nanos / 1_000_000_000) as u64,
        (nanos % 1_000_000_000) as u32,
    )
}

#[cfg(test)]
#[path = "navigation_tests.rs"]
mod tests;
