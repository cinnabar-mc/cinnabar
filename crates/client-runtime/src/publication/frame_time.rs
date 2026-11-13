//! Frame-time feedback: which render frames are slow against this device's own normal.

use std::{collections::VecDeque, time::Duration};

use render_api::FrameTimePressureConfig;
use render_scene::RenderFrameSample;

/// Render-frame samples kept out of the typical time after a main frame admitted more than
/// the pressure floors: the sample already in flight when that frame ended, the render frame
/// that carries its uploads, and one more for uploads the renderer deferred.
const UPLOAD_SETTLE_FRAMES: u32 = 3;

/// What the newest render frame says about publication pressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FrameTimeSignal {
    /// No new render frame arrived, or the new one was not slow.
    Healthy,
    /// A slow frame that does not complete a run.
    Slow,
    /// The frame that completes a run of consecutive slow frames.
    Sustained,
}

/// Judges render frames against the hitch threshold and the device's typical frame time.
///
/// The typical time is the rolling median of recent render frames that followed light
/// publication: at most the pressure floors' worth of payloads. Frames after heavier
/// publication never enter it, so a long upload burst cannot raise the typical time and
/// hide itself, however long it lasts. Once back-off has reduced publication to the floors,
/// frames feed the median again; a slowdown that back-off does not cure is then learned as
/// the new normal and service recovers.
#[derive(Debug, Clone)]
pub(super) struct FrameTimeFeedback {
    config: FrameTimePressureConfig,
    /// Newest eligible frame times in microseconds, oldest first.
    samples: VecDeque<u32>,
    /// Reused to select the median without allocating each frame.
    scratch: Vec<u32>,
    /// Median of `samples` once the warm-up count is reached.
    typical: Option<Duration>,
    /// Sequence of the newest render frame counted, so a repeated sample adds nothing.
    observed_render_frame: Option<u64>,
    /// Consecutive slow render frames ending at the newest sample.
    slow_render_frames: u32,
    /// Whether a render frame arrived since the last signal was taken.
    pending: bool,
    /// Render frames still to keep out of the typical time after heavy publication.
    upload_settle_frames: u32,
}

impl FrameTimeFeedback {
    /// Allocates the sample window once, so later frames allocate nothing.
    pub(super) fn new(config: FrameTimePressureConfig) -> Self {
        Self {
            config,
            samples: VecDeque::with_capacity(config.baseline_frames),
            scratch: Vec::with_capacity(config.baseline_frames),
            typical: None,
            observed_render_frame: None,
            slow_render_frames: 0,
            pending: false,
            upload_settle_frames: 0,
        }
    }

    /// The typical render-frame time, or `None` until enough eligible frames arrived.
    pub(super) fn typical(&self) -> Option<Duration> {
        self.typical
    }

    /// Records the payloads one finished main frame admitted. Call it once per main frame,
    /// before observing the render frame that follows.
    pub(super) fn note_admissions(&mut self, items: usize, bytes: u64) {
        self.upload_settle_frames =
            if items > self.config.minimum_frame_items || bytes > self.config.minimum_frame_bytes {
                UPLOAD_SETTLE_FRAMES
            } else {
                self.upload_settle_frames.saturating_sub(1)
            };
    }

    /// Counts one completed render frame. A sample whose sequence was already counted adds
    /// nothing, so callers may pass the newest sample every frame.
    pub(super) fn observe(&mut self, sample: RenderFrameSample) {
        if self.observed_render_frame == Some(sample.sequence) {
            return;
        }
        self.observed_render_frame = Some(sample.sequence);
        self.pending = true;
        // Judged against the normal before this frame, so a spike cannot vouch for itself.
        self.slow_render_frames = if self.is_slow(sample) {
            self.slow_render_frames.saturating_add(1)
        } else {
            0
        };
        if self.upload_settle_frames == 0 {
            self.record_typical(sample.elapsed);
        }
    }

    /// Returns the newest render frame's signal once; later calls report `Healthy` until
    /// another frame is observed.
    pub(super) fn take_signal(&mut self) -> FrameTimeSignal {
        if !std::mem::take(&mut self.pending) || self.slow_render_frames == 0 {
            FrameTimeSignal::Healthy
        } else if self
            .slow_render_frames
            .is_multiple_of(self.config.consecutive_slow_frames)
        {
            FrameTimeSignal::Sustained
        } else {
            FrameTimeSignal::Slow
        }
    }

    /// Whether a frame exceeds the hitch threshold and the margin over the typical time.
    /// Before a typical time exists no frame is slow: the device's normal is unknown, and
    /// the fixed per-frame caps still bound uploads.
    fn is_slow(&self, sample: RenderFrameSample) -> bool {
        sample.over_hitch()
            && self.typical.is_some_and(|typical| {
                sample.elapsed.as_nanos().saturating_mul(100)
                    > typical
                        .as_nanos()
                        .saturating_mul(u128::from(self.config.slow_frame_percent))
            })
    }

    /// Adds an eligible frame time to the window and refreshes the median.
    fn record_typical(&mut self, elapsed: Duration) {
        if self.samples.len() >= self.config.baseline_frames {
            self.samples.pop_front();
        }
        self.samples
            .push_back(u32::try_from(elapsed.as_micros()).unwrap_or(u32::MAX));
        if self.samples.len() < self.config.baseline_warmup_frames {
            return;
        }
        self.scratch.clear();
        self.scratch.extend(self.samples.iter().copied());
        let middle = self.scratch.len() / 2;
        let (_, median, _) = self.scratch.select_nth_unstable(middle);
        self.typical = Some(Duration::from_micros(u64::from(*median)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: FrameTimePressureConfig = FrameTimePressureConfig {
        consecutive_slow_frames: 3,
        baseline_frames: 9,
        baseline_warmup_frames: 3,
        slow_frame_percent: 150,
        minimum_frame_items: 4,
        minimum_frame_bytes: 1024,
    };
    const HITCH: Duration = Duration::from_micros(12_500);

    /// Feeds frame times as consecutive render frames after light publication.
    fn observe(feedback: &mut FrameTimeFeedback, sequence: &mut u64, millis: &[u64]) {
        for &millis in millis {
            *sequence += 1;
            feedback.note_admissions(0, 0);
            feedback.observe(RenderFrameSample {
                sequence: *sequence,
                elapsed: Duration::from_millis(millis),
                hitch: HITCH,
            });
        }
    }

    /// The typical time is the median of the newest window, unmoved by a few outliers.
    #[test]
    fn the_typical_time_is_the_median_of_the_newest_window() {
        let mut feedback = FrameTimeFeedback::new(CONFIG);
        let mut sequence = 0;
        observe(&mut feedback, &mut sequence, &[22, 22]);
        assert_eq!(feedback.typical(), None, "below the warm-up count");
        observe(&mut feedback, &mut sequence, &[22]);
        assert_eq!(feedback.typical(), Some(Duration::from_millis(22)));

        observe(&mut feedback, &mut sequence, &[200, 300, 22, 21, 23, 22]);
        assert_eq!(feedback.typical(), Some(Duration::from_millis(22)));

        // Nine newer frames replace the whole window.
        observe(&mut feedback, &mut sequence, &[30; 9]);
        assert_eq!(feedback.typical(), Some(Duration::from_millis(30)));
    }

    /// A frame is slow only above both the hitch threshold and the margin over typical.
    #[test]
    fn a_frame_is_slow_only_above_the_hitch_and_the_margin_over_typical() {
        let mut feedback = FrameTimeFeedback::new(CONFIG);
        let mut sequence = 0;
        // Before a typical time exists, no frame is slow.
        observe(&mut feedback, &mut sequence, &[400]);
        assert_eq!(feedback.take_signal(), FrameTimeSignal::Healthy);

        observe(&mut feedback, &mut sequence, &[22; 8]);
        assert_eq!(feedback.take_signal(), FrameTimeSignal::Healthy);
        assert_eq!(feedback.typical(), Some(Duration::from_millis(22)));
        // 33 ms is exactly 150% of typical: not slow. 34 ms is.
        observe(&mut feedback, &mut sequence, &[33]);
        assert_eq!(feedback.take_signal(), FrameTimeSignal::Healthy);
        observe(&mut feedback, &mut sequence, &[34]);
        assert_eq!(feedback.take_signal(), FrameTimeSignal::Slow);

        // Under the hitch threshold nothing is slow, whatever the typical time.
        let mut fast = FrameTimeFeedback::new(CONFIG);
        observe(&mut fast, &mut sequence, &[2, 2, 2, 12]);
        assert_eq!(fast.take_signal(), FrameTimeSignal::Healthy);
    }

    /// Every run of consecutive slow frames reports pressure once, and a signal is taken once.
    #[test]
    fn each_run_of_consecutive_slow_frames_is_sustained_once() {
        let mut feedback = FrameTimeFeedback::new(CONFIG);
        let mut sequence = 0;
        observe(&mut feedback, &mut sequence, &[10, 10, 10]);
        let mut signals = Vec::new();
        for _ in 0..7 {
            // Heavy publication keeps these frames out of the typical time.
            sequence += 1;
            feedback.note_admissions(CONFIG.minimum_frame_items + 1, 0);
            feedback.observe(RenderFrameSample {
                sequence,
                elapsed: Duration::from_millis(40),
                hitch: HITCH,
            });
            signals.push(feedback.take_signal());
            assert_eq!(feedback.take_signal(), FrameTimeSignal::Healthy);
        }
        use FrameTimeSignal::{Slow, Sustained};
        assert_eq!(
            signals,
            [Slow, Slow, Sustained, Slow, Slow, Sustained, Slow]
        );
        assert_eq!(feedback.typical(), Some(Duration::from_millis(10)));
    }

    /// Frames after heavy publication stay out of the typical time until uploads settle;
    /// frames after light publication enter it even when they are slow.
    #[test]
    fn only_frames_after_light_publication_feed_the_typical_time() {
        let mut feedback = FrameTimeFeedback::new(CONFIG);
        let mut sequence = 0;
        observe(&mut feedback, &mut sequence, &[10; 9]);

        feedback.note_admissions(0, CONFIG.minimum_frame_bytes + 1);
        for _ in 0..UPLOAD_SETTLE_FRAMES {
            sequence += 1;
            feedback.observe(RenderFrameSample {
                sequence,
                elapsed: Duration::from_millis(90),
                hitch: HITCH,
            });
            feedback.note_admissions(0, 0);
        }
        assert_eq!(feedback.samples.iter().max(), Some(&10_000));

        // At the floors exactly, publication is light.
        feedback.note_admissions(CONFIG.minimum_frame_items, CONFIG.minimum_frame_bytes);
        observe(&mut feedback, &mut sequence, &[90; 5]);
        assert_eq!(feedback.typical(), Some(Duration::from_millis(90)));
    }
}
