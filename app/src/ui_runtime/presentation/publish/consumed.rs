//! The runtime revisions a preparation step last read.

use bevy::ecs::change_detection::Tick;

/// The player and UI runtime change ticks a step last read them at. Unlike the system's own
/// last run, it advances only when the step runs, so a frame that returns before reaching the
/// step never consumes a change.
#[derive(Default)]
pub(crate) struct ConsumedTicks(Option<(Tick, Tick)>);

impl ConsumedTicks {
    /// Whether neither runtime changed since the step last recorded its read.
    pub(crate) fn unchanged(&self, player: Tick, runtime: Tick) -> bool {
        self.0 == Some((player, runtime))
    }

    /// Records the runtimes as the step just read them, during the run at `this_run`. A write
    /// earlier in the same run shares the tick of any later write, which the step did not read,
    /// so such a read is not recorded and the step runs again next frame.
    pub(crate) fn record(&mut self, player: Tick, runtime: Tick, this_run: Tick) {
        self.0 = (player != this_run && runtime != this_run).then_some((player, runtime));
    }
}
