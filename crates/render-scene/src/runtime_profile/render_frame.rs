//! Render-thread frame time for consumers that pace work by it.

use super::RuntimeStageProfiler;
use crate::RenderFrameSample;

impl RuntimeStageProfiler {
    /// Returns the newest render-thread frame from the always-on gameplay recorder, or
    /// `None` when slow-frame attribution is off or no render frame has completed.
    #[must_use]
    pub fn latest_render_frame(&self) -> Option<RenderFrameSample> {
        self.state.slow.as_ref()?.latest_render_frame()
    }
}

#[cfg(test)]
mod tests {
    use crate::{FrameBudgets, RuntimeStage, RuntimeStageProfiler};
    use std::time::Duration;

    /// Each completed render frame is a new sample judged against the display's hitch.
    #[test]
    fn latest_render_frame_reports_each_completed_frame_against_the_hitch() {
        let profiler = RuntimeStageProfiler::for_gameplay(false, None);
        assert_eq!(profiler.latest_render_frame(), None);
        let interval = Duration::from_micros(8_333);
        profiler.set_frame_interval(interval);
        let hitch = FrameBudgets::for_interval(interval).hitch;

        profiler.record_background_sample(RuntimeStage::RenderFrame, hitch * 2);
        let first = profiler.latest_render_frame().unwrap();
        assert_eq!(first.elapsed, hitch * 2);
        assert_eq!(first.hitch, hitch);
        assert!(first.over_hitch());
        assert_eq!(profiler.latest_render_frame(), Some(first));

        // Other stages are not render frames.
        profiler.record_background_sample(RuntimeStage::RenderSubmission, hitch * 2);
        assert_eq!(profiler.latest_render_frame(), Some(first));

        profiler.record_background_sample(RuntimeStage::RenderFrame, hitch);
        let second = profiler.latest_render_frame().unwrap();
        assert_ne!(second.sequence, first.sequence);
        assert_eq!(second.elapsed, hitch);
        assert!(!second.over_hitch());

        assert_eq!(RuntimeStageProfiler::new(true).latest_render_frame(), None);
    }
}
