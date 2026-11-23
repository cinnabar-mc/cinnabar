//! The clock a frame build reads, which records how long the built frame stays current.
//!
//! A build reads the time only through [`FrameClock`]. Reading the raw time marks the frame as
//! changing next frame; the bounded queries record the instant their answer next changes. The
//! earliest such instant is how long the frame, with every other input unchanged, would be built
//! again exactly.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

/// One build's clock, in milliseconds.
#[derive(Debug)]
pub struct FrameClock {
    now: u64,
    /// `now` in seconds; tests may set sub-millisecond times.
    now_seconds: f64,
    /// The frame repeats while the clock reads less than this; [`u64::MAX`] holds forever.
    /// Atomic only so the presentation stays `Sync`; one thread builds a frame.
    holds_until: AtomicU64,
    /// Where the read that set `holds_until` happened, for diagnostics.
    #[cfg(debug_assertions)]
    changed_by: std::sync::Mutex<Option<&'static std::panic::Location<'static>>>,
}

impl Clone for FrameClock {
    fn clone(&self) -> Self {
        Self {
            now: self.now,
            now_seconds: self.now_seconds,
            holds_until: AtomicU64::new(self.holds_until.load(Relaxed)),
            #[cfg(debug_assertions)]
            changed_by: std::sync::Mutex::new(self.changed_by()),
        }
    }
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new(0)
    }
}

impl FrameClock {
    /// A clock at `now_millis` that nothing has read yet.
    pub const fn new(now_millis: u64) -> Self {
        Self {
            now: now_millis,
            now_seconds: now_millis as f64 / 1_000.0,
            holds_until: AtomicU64::new(u64::MAX),
            #[cfg(debug_assertions)]
            changed_by: std::sync::Mutex::new(None),
        }
    }

    /// The time itself. Whatever reads it may draw differently next frame.
    #[track_caller]
    pub fn millis(&self) -> u64 {
        self.changes_every_frame();
        self.now
    }

    /// [`Self::millis`] in seconds.
    #[track_caller]
    pub fn seconds(&self) -> f64 {
        self.changes_every_frame();
        self.now_seconds
    }

    /// A clock at exactly `seconds`, for tests that step screens by fractions of a millisecond.
    #[cfg(any(test, feature = "test-support"))]
    pub fn at_seconds(seconds: f64) -> Self {
        Self {
            now_seconds: seconds,
            ..Self::new((seconds * 1_000.0) as u64)
        }
    }

    /// Whether `at` is still ahead; the answer holds until `at`.
    #[track_caller]
    pub fn before(&self, at: u64) -> bool {
        if self.now < at {
            self.holds_until(at);
            true
        } else {
            false
        }
    }

    /// Whether the clock lies in `[start, end)`; the answer holds until the next edge ahead.
    #[track_caller]
    pub fn within(&self, start: u64, end: u64) -> bool {
        if self.now < start {
            self.holds_until(start);
            false
        } else {
            self.before(end)
        }
    }

    /// Whole `step`s elapsed since `origin` (zero before it); the count holds until the next
    /// step boundary.
    #[track_caller]
    pub fn steps_since(&self, origin: u64, step: u64) -> u64 {
        let step = step.max(1);
        if self.now < origin {
            self.holds_until(origin);
            return 0;
        }
        let steps = (self.now - origin) / step;
        self.holds_until(
            origin
                .saturating_add(steps.saturating_mul(step))
                .saturating_add(step),
        );
        steps
    }

    /// Records that what was read so far draws differently from `at` on.
    #[track_caller]
    pub fn holds_until(&self, at: u64) {
        if at < self.holds_until.load(Relaxed) {
            self.holds_until.store(at, Relaxed);
            #[cfg(debug_assertions)]
            {
                *self
                    .changed_by
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner()) =
                    Some(std::panic::Location::caller());
            }
        }
    }

    /// Records that what was read so far may draw differently on every frame.
    #[track_caller]
    pub fn changes_every_frame(&self) {
        self.holds_until(self.now);
    }

    /// The time for a JSON-UI paint, whose animations read it. Report through
    /// [`EngineTime::settled`] whether the paint's output holds; a paint that does not report is
    /// taken to change every frame.
    pub fn engine(&self) -> EngineTime<'_> {
        EngineTime {
            clock: self,
            reported: Cell::new(false),
        }
    }

    /// Whether a frame built at this clock is still current at `now_millis`.
    pub fn holds_at(&self, now_millis: u64) -> bool {
        now_millis < self.holds_until.load(Relaxed)
    }

    /// Where the read that bounds how long the frame holds happened, for diagnostics.
    #[cfg(debug_assertions)]
    pub fn changed_by(&self) -> Option<&'static std::panic::Location<'static>> {
        *self
            .changed_by
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

/// The time handed to one JSON-UI paint, until the paint reports whether its output holds.
#[must_use = "an unreported paint is taken to change every frame"]
pub struct EngineTime<'a> {
    clock: &'a FrameClock,
    reported: Cell<bool>,
}

impl EngineTime<'_> {
    /// The paint's time in seconds.
    pub fn seconds(&self) -> f64 {
        self.clock.now_seconds
    }

    /// Reports whether the paint's output holds while its other inputs do: no animation it
    /// drew or ticked is still running.
    #[track_caller]
    pub fn settled(self, settled: bool) {
        self.reported.set(true);
        if !settled {
            self.clock.changes_every_frame();
        }
    }
}

impl Drop for EngineTime<'_> {
    fn drop(&mut self) {
        if !self.reported.get() {
            self.clock.changes_every_frame();
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl super::UiPresentationRuntime {
    /// Sets the clock screens paint at, for tests that drive screens without a build.
    pub fn set_menu_seconds(&mut self, seconds: f64) {
        self.clock = FrameClock::at_seconds(seconds);
    }

    /// The clock screens paint at, in seconds.
    pub fn menu_seconds(&self) -> f64 {
        self.clock.now_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untouched_clock_holds_forever_and_raw_reads_change_next_frame() {
        let clock = FrameClock::new(100);
        assert!(clock.holds_at(u64::MAX - 1));
        assert_eq!(clock.millis(), 100);
        assert!(
            !clock.holds_at(100),
            "a raw read rebuilds even within the same millisecond"
        );
    }

    #[test]
    fn bounded_reads_hold_until_their_answer_changes() {
        let clock = FrameClock::new(100);
        assert!(clock.before(150));
        assert!(!clock.within(120, 130));
        assert!(clock.holds_at(119) && !clock.holds_at(120));
        let clock = FrameClock::new(100);
        assert_eq!(clock.steps_since(10, 50), 1);
        assert!(clock.holds_at(109) && !clock.holds_at(110));
        let clock = FrameClock::new(100);
        assert!(!clock.before(100) && clock.holds_at(u64::MAX - 1));
    }

    #[test]
    fn unreported_paints_change_every_frame() {
        let clock = FrameClock::new(5);
        clock.engine().settled(true);
        assert!(clock.holds_at(6));
        let _ = clock.engine();
        assert!(!clock.holds_at(5));
        let clock = FrameClock::new(5);
        clock.engine().settled(false);
        assert!(!clock.holds_at(5));
    }
}
