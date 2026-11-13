//! Work counted per render frame: fixed atomics while counting, one lock per frame boundary,
//! formatted only in reports.

use super::RuntimeStageProfiler;
use std::{
    fmt::Write,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

/// Work a render frame did, counted on any thread where client code asks the GPU API for it.
/// Bevy's own passes and buffer writes are not counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum RenderCounter {
    /// Render and compute passes begun.
    Passes,
    /// `Queue::write_buffer` and `write_texture` calls.
    Writes,
    /// Bytes those queue writes carried.
    WriteBytes,
    /// Bytes copied through retained staging buffers instead of the queue.
    StagedBytes,
}

const COUNTERS: usize = RenderCounter::ALL.len();

impl RenderCounter {
    pub const ALL: [Self; 4] = [
        Self::Passes,
        Self::Writes,
        Self::WriteBytes,
        Self::StagedBytes,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Passes => "passes",
            Self::Writes => "writes",
            Self::WriteBytes => "write_bytes",
            Self::StagedBytes => "staged_bytes",
        }
    }
}

/// One counter per [`RenderCounter`], in [`RenderCounter::ALL`] order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderCounts(pub [u64; COUNTERS]);

impl RenderCounts {
    #[must_use]
    pub fn get(&self, counter: RenderCounter) -> u64 {
        self.0[counter as usize]
    }

    /// Whether nothing was counted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0 == [0; COUNTERS]
    }

    /// Appends ` render_counts=name:value,...` with every counter, zero or not.
    pub(crate) fn append_fields(&self, line: &mut String) {
        line.push_str(" render_counts=");
        let mut separator = "";
        for counter in RenderCounter::ALL {
            let _ = write!(line, "{separator}{}:{}", counter.name(), self.get(counter));
            separator = ",";
        }
    }
}

/// Counts shared by every thread, split at render-frame boundaries.
struct Counters {
    /// Counted since the render frame last closed.
    pending: [AtomicU64; COUNTERS],
    /// Everything counted by closed render frames. One lock publishes a frame's counters
    /// together, so a reader never sees part of a frame.
    closed: Mutex<[u64; COUNTERS]>,
}

impl Counters {
    const fn new() -> Self {
        Self {
            pending: [const { AtomicU64::new(0) }; COUNTERS],
            closed: Mutex::new([0; COUNTERS]),
        }
    }

    fn add(&self, counter: RenderCounter, amount: u64) {
        self.pending[counter as usize].fetch_add(amount, Ordering::Relaxed);
    }

    /// Everything counted by the render frames closed so far.
    fn closed(&self) -> [u64; COUNTERS] {
        *self.closed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Ends a render frame and returns its counts.
    fn close_frame(&self) -> RenderCounts {
        let frame: [u64; COUNTERS] =
            std::array::from_fn(|index| self.pending[index].swap(0, Ordering::Relaxed));
        let mut closed = self.closed.lock().unwrap_or_else(PoisonError::into_inner);
        for (total, counted) in closed.iter_mut().zip(frame) {
            *total = total.wrapping_add(counted);
        }
        RenderCounts(frame)
    }

    /// Counts of the render frames closed since `window` last read them.
    fn take_window(&self, window: &RenderCountWindow) -> RenderCounts {
        let closed = self.closed();
        let mut reported = window.0.lock().unwrap_or_else(PoisonError::into_inner);
        let counts = std::array::from_fn(|index| closed[index].wrapping_sub(reported[index]));
        *reported = closed;
        RenderCounts(counts)
    }
}

static COUNTS: Counters = Counters::new();

/// One profiler's position in the closed render frames, advanced at its main frame boundaries.
#[derive(Debug)]
pub(super) struct RenderCountWindow(Mutex<[u64; COUNTERS]>);

impl RenderCountWindow {
    /// Starts after every render frame already closed.
    pub(super) fn new() -> Self {
        Self::after(&COUNTS)
    }

    fn after(counters: &Counters) -> Self {
        Self(crate::runtime_profile::initialized_mutex(counters.closed()))
    }

    /// Counts of the render frames that closed since the previous main frame boundary.
    pub(super) fn take(&self) -> RenderCounts {
        COUNTS.take_window(self)
    }
}

/// Adds `amount` to `counter` for the render frame in progress, from any thread. Costs one
/// relaxed atomic add and never allocates.
#[inline]
pub fn count(counter: RenderCounter, amount: u64) {
    COUNTS.add(counter, amount);
}

impl RuntimeStageProfiler {
    /// Closes the render frame's counters on the render thread and marks them in the frame
    /// trace. Returns what the frame counted.
    pub fn finish_render_counters(&self) -> RenderCounts {
        let counts = COUNTS.close_frame();
        if !counts.is_empty()
            && let Some(trace) = &self.state.trace
        {
            trace.render_counts(self.frame_id(), counts);
        }
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A render frame's counts reach exactly one main-frame window.
    #[test]
    fn closed_render_frames_are_reported_once() {
        let counters = Counters::new();
        let window = RenderCountWindow::after(&counters);
        counters.add(RenderCounter::Passes, 3);
        counters.add(RenderCounter::WriteBytes, 64);
        assert!(counters.take_window(&window).is_empty(), "frame still open");
        let frame = counters.close_frame();
        assert_eq!(frame.get(RenderCounter::Passes), 3);
        assert_eq!(frame.get(RenderCounter::WriteBytes), 64);
        counters.add(RenderCounter::Passes, 2);
        assert_eq!(counters.take_window(&window), frame);
        assert!(counters.take_window(&window).is_empty());
        counters.close_frame();
        counters.close_frame();
        assert_eq!(counters.take_window(&window).get(RenderCounter::Passes), 2);
        let late = RenderCountWindow::after(&counters);
        assert!(counters.take_window(&late).is_empty(), "earlier frames");
    }

    /// Counting on the hot path must not allocate.
    #[test]
    fn counting_does_not_allocate() {
        let (_, allocations) = test_alloc::count(|| {
            for counter in RenderCounter::ALL {
                count(counter, 1);
            }
        });
        assert_eq!(allocations, 0);
    }

    #[test]
    fn report_fields_name_every_counter() {
        let mut counts = RenderCounts::default();
        counts.0[RenderCounter::Writes as usize] = 7;
        let mut line = String::new();
        counts.append_fields(&mut line);
        assert!(line.starts_with(" render_counts="), "{line}");
        for counter in RenderCounter::ALL {
            let expected = format!("{}:{}", counter.name(), counts.get(counter));
            assert!(line.contains(&expected), "{line}");
        }
    }
}
