//! The resource revisions a preparation step last read.

use bevy::ecs::change_detection::Tick;

/// The change ticks of `N` resources a step last read them at. Unlike the system's own last
/// run, it advances only when the step runs, so a frame that returns before reaching the step
/// never consumes a change.
pub(crate) struct ConsumedTicks<const N: usize>(Option<[Tick; N]>);

impl<const N: usize> Default for ConsumedTicks<N> {
    fn default() -> Self {
        Self(None)
    }
}

impl<const N: usize> ConsumedTicks<N> {
    /// Whether none of the resources changed since the step last recorded its read.
    pub(crate) fn unchanged(&self, ticks: [Tick; N]) -> bool {
        self.0 == Some(ticks)
    }

    /// Records the resources as the step just read them, during the run at `this_run`. A write
    /// earlier in the same run shares the tick of any later write, which the step did not read,
    /// so such a read is not recorded and the step runs again next frame.
    pub(crate) fn record(&mut self, ticks: [Tick; N], this_run: Tick) {
        self.0 = (!ticks.contains(&this_run)).then_some(ticks);
    }
}
