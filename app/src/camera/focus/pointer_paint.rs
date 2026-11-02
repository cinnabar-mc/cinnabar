//! Decides when pointer moves must stop holding back the next frame on Windows.
//!
//! The event loop starts frames from `WM_PAINT` and drains the message queue until it reads empty.
//! Windows hands out `WM_PAINT` only when no input is queued, and it produces a fresh `WM_MOUSEMOVE`
//! whenever the cursor has moved since the last one was taken. A move at 8 kHz arrives every 125 us,
//! so once handling a move takes longer than that, the queue never reads empty and no frame starts
//! until the mouse stops. Slower reports or cheaper handling let the queue empty, and then this
//! never triggers.
use std::time::{Duration, Instant};

/// How long pointer moves may run after a frame before the next frame goes first.
///
/// Long enough for the few messages queued during a frame, so a click stays in the next frame,
/// and short enough that moves take a small part of a frame's time however fast the mouse reports.
const POINTER_DRAIN_BUDGET: Duration = Duration::from_millis(1);

/// Tracks how long pointer moves have been running since the last frame.
#[derive(Clone, Copy)]
pub(super) struct PaintTurn {
    since: Option<Instant>,
}

impl PaintTurn {
    /// Starts with no frame recorded, before the window's first paint.
    pub(super) const fn new() -> Self {
        Self { since: None }
    }

    /// Records that a frame just finished, which starts a new budget.
    pub(super) fn frame_finished(&mut self, now: Instant) {
        self.since = Some(now);
    }

    /// Reports whether a pending frame should run now that a pointer move has been handled.
    ///
    /// A move with no frame recorded starts the budget, so the first frame cannot be held back
    /// either.
    pub(super) fn frame_due_after_move(&mut self, now: Instant) -> bool {
        let since = *self.since.get_or_insert(now);
        now.saturating_duration_since(since) >= POINTER_DRAIN_BUDGET
    }
}
