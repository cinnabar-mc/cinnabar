//! Presentation-only admission and midpoint deadlines; callers supply monotonic nanoseconds.

/// Frame generation is opt-in for every saved profile and device.
pub const DEFAULT_FRAME_GENERATION: bool = false;
/// Interpolation is bypassed below this real-frame rate.
pub const FRAME_GENERATION_MIN_FPS: u64 = 45;
/// A late candidate never extends presentation by more than this fraction of a real interval.
const LATE_DIVISOR: u64 = 8;
/// Real frames presented without interpolation after a missed midpoint, so an overloaded
/// device returns to ordinary pipelined rendering instead of alternating every frame.
pub const MISSED_DEADLINE_BACKOFF_FRAMES: u32 = 60;
/// Base rate used for the latency example in the setting's description.
const DESCRIPTION_EXAMPLE_FPS: u64 = 60;
const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// Player-facing description; the latency bound follows from holding at most one real interval.
#[must_use]
pub fn frame_generation_description() -> String {
    let example_ms = (NANOS_PER_SECOND / DESCRIPTION_EXAMPLE_FPS).div_ceil(1_000_000);
    format!(
        "Shows an interpolated world frame between real frames. Adds up to one real frame of \
         input latency ({example_ms} ms at {DESCRIPTION_EXAMPLE_FPS} FPS). Needs a frame-rate \
         limit or known refresh rate and {FRAME_GENERATION_MIN_FPS} real FPS. Pauses with motion \
         blur, Enhanced rendering or shader packs."
    )
}

/// Real-frame period the presenter paces: the frame-rate limit, but never shorter than two
/// display refreshes so each generated image can reach the screen. `None` means nothing paces
/// real frames, so an extra presentation would only halve the real rate.
#[must_use]
pub fn real_interval_ns(limit_ns: Option<u64>, refresh_ns: Option<u64>) -> Option<u64> {
    let refreshes = refresh_ns.map(|refresh| refresh.saturating_mul(2));
    match (limit_ns, refreshes) {
        (None, None) => None,
        (limit, refreshes) => Some(limit.unwrap_or(0).max(refreshes.unwrap_or(0))),
    }
    .filter(|interval| *interval > 0)
}

/// Conditions that make an extra presentation unsafe or unhelpful.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameGenerationBypass {
    Disabled,
    Preparing,
    Unsupported,
    Capture,
    Discontinuity,
    Unpaced,
    LowFrameRate,
    MissedDeadline,
}

/// Current real-frame facts. None of these values advances simulation or samples input.
#[derive(Clone, Copy, Debug)]
pub struct FrameGenerationAdmission {
    pub enabled: bool,
    pub prepared: bool,
    pub compatible: bool,
    pub capture: bool,
    pub continuous: bool,
    /// From [`real_interval_ns`].
    pub interval_ns: Option<u64>,
}

impl FrameGenerationAdmission {
    /// Rejects a candidate before encoding interpolation or delaying a real frame.
    #[must_use]
    pub fn bypass(self) -> Option<FrameGenerationBypass> {
        use FrameGenerationBypass::*;
        if !self.enabled {
            return Some(Disabled);
        }
        if !self.prepared {
            return Some(Preparing);
        }
        if !self.compatible {
            return Some(Unsupported);
        }
        if self.capture {
            return Some(Capture);
        }
        if !self.continuous {
            return Some(Discontinuity);
        }
        let Some(interval) = self.interval_ns else {
            return Some(Unpaced);
        };
        if interval > NANOS_PER_SECOND.div_ceil(FRAME_GENERATION_MIN_FPS) {
            return Some(LowFrameRate);
        }
        None
    }
}

/// When to present a ready candidate, relative to the last real presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameGenerationPlan {
    /// No anchored real presentation: present the real image now and anchor on it.
    Warmup,
    /// Present the generated image, then the real image, at these deadlines.
    Pair {
        generated_ns: u64,
        real_ns: u64,
    },
    Bypass(FrameGenerationBypass),
}

/// Anchors an interpolated image to the last successful real presentation.
#[derive(Default, Debug)]
pub struct FrameGenerationPacer {
    real_ns: Option<u64>,
    backoff: u32,
}

impl FrameGenerationPacer {
    /// A bypass, surface change or failed present must never bridge an older pair.
    pub fn reset(&mut self) {
        self.real_ns = None;
    }

    /// Whether this real frame may be held for interpolation; consumes one backoff frame.
    pub fn admit(&mut self) -> bool {
        if self.backoff == 0 {
            return true;
        }
        self.backoff -= 1;
        self.reset();
        false
    }

    /// Drops the anchor and keeps the following real frames ordinary.
    pub fn missed(&mut self) {
        self.reset();
        self.backoff = MISSED_DEADLINE_BACKOFF_FRAMES;
    }

    /// Records a real presentation that completed while interpolation was admitted.
    pub fn presented_real(&mut self, at_ns: u64) {
        self.real_ns = Some(at_ns);
    }

    /// Schedules a ready candidate halfway between real presentations without reading the clock.
    pub fn plan(&mut self, ready_ns: u64, interval_ns: u64) -> FrameGenerationPlan {
        let Some(previous) = self.real_ns else {
            return FrameGenerationPlan::Warmup;
        };
        let midpoint = previous.saturating_add(interval_ns / 2);
        if ready_ns < previous || ready_ns > midpoint.saturating_add(interval_ns / LATE_DIVISOR) {
            self.missed();
            return FrameGenerationPlan::Bypass(FrameGenerationBypass::MissedDeadline);
        }
        let generated_ns = ready_ns.max(midpoint);
        FrameGenerationPlan::Pair {
            generated_ns,
            real_ns: self
                .real_deadline(generated_ns, interval_ns)
                .unwrap_or(generated_ns),
        }
    }

    /// Keeps the generated image midway after its actual presentation time is known, but never
    /// holds the real image past one interval plus the late allowance after the previous one.
    #[must_use]
    pub fn real_deadline(&self, generated_ns: u64, interval_ns: u64) -> Option<u64> {
        self.real_ns.map(|previous| {
            let midway = generated_ns.saturating_add(generated_ns.saturating_sub(previous));
            let latest = previous
                .saturating_add(interval_ns)
                .saturating_add(interval_ns / LATE_DIVISOR);
            midway.min(latest)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Makes one admission fact ineligible.
    type Mutation = fn(&mut FrameGenerationAdmission);

    /// Supplies an otherwise eligible 60 FPS real frame.
    fn eligible() -> FrameGenerationAdmission {
        FrameGenerationAdmission {
            enabled: true,
            prepared: true,
            compatible: true,
            capture: false,
            continuous: true,
            interval_ns: Some(16_000_000),
        }
    }

    #[test]
    fn runtime_bypasses_preserve_ordinary_real_frame_admission() {
        assert_eq!(eligible().bypass(), None);
        let cases: [(FrameGenerationBypass, Mutation); 7] = [
            (FrameGenerationBypass::Disabled, |f| f.enabled = false),
            (FrameGenerationBypass::Preparing, |f| f.prepared = false),
            (FrameGenerationBypass::Unsupported, |f| f.compatible = false),
            (FrameGenerationBypass::Capture, |f| f.capture = true),
            (FrameGenerationBypass::Discontinuity, |f| {
                f.continuous = false
            }),
            (FrameGenerationBypass::Unpaced, |f| f.interval_ns = None),
            (FrameGenerationBypass::LowFrameRate, |f| {
                f.interval_ns = Some(30_000_000)
            }),
        ];
        for (reason, apply) in cases {
            let mut facts = eligible();
            apply(&mut facts);
            assert_eq!(facts.bypass(), Some(reason));
        }
        let mut boundary = eligible();
        boundary.interval_ns = Some(NANOS_PER_SECOND.div_ceil(FRAME_GENERATION_MIN_FPS));
        assert_eq!(
            boundary.bypass(),
            None,
            "nanosecond rounding admits the threshold"
        );
    }

    #[test]
    fn real_interval_follows_the_limit_and_leaves_room_for_two_refreshes() {
        assert_eq!(real_interval_ns(None, None), None, "unpaced");
        assert_eq!(real_interval_ns(Some(16_000_000), None), Some(16_000_000));
        assert_eq!(real_interval_ns(None, Some(8_000_000)), Some(16_000_000));
        assert_eq!(
            real_interval_ns(Some(10_000_000), Some(8_000_000)),
            Some(16_000_000),
            "a limit above half the refresh rate still presents both images"
        );
        assert_eq!(
            real_interval_ns(Some(20_000_000), Some(8_000_000)),
            Some(20_000_000)
        );
        assert_eq!(real_interval_ns(Some(0), Some(0)), None);
    }

    #[test]
    fn ready_candidates_are_midpoints_and_late_candidates_back_off() {
        let mut pacer = FrameGenerationPacer::default();
        assert!(pacer.admit());
        assert_eq!(pacer.plan(100, 16), FrameGenerationPlan::Warmup);
        pacer.presented_real(100);
        assert_eq!(
            pacer.plan(104, 16),
            FrameGenerationPlan::Pair {
                generated_ns: 108,
                real_ns: 116
            },
            "an early candidate waits for the midpoint"
        );
        assert_eq!(
            pacer.plan(110, 16),
            FrameGenerationPlan::Pair {
                generated_ns: 110,
                real_ns: 118
            },
            "a slightly late candidate delays its real frame, at most by the late allowance"
        );
        assert_eq!(pacer.real_deadline(109, 16), Some(118));
        assert_eq!(
            pacer.plan(111, 16),
            FrameGenerationPlan::Bypass(FrameGenerationBypass::MissedDeadline)
        );
        for _ in 0..MISSED_DEADLINE_BACKOFF_FRAMES {
            assert!(!pacer.admit(), "a miss keeps following frames ordinary");
        }
        assert!(pacer.admit());
        assert_eq!(pacer.plan(200, 16), FrameGenerationPlan::Warmup);
    }

    #[test]
    fn steady_pacing_keeps_the_target_interval_without_drifting_slower() {
        let interval = 16_000_000;
        let mut pacer = FrameGenerationPacer::default();
        assert_eq!(pacer.plan(0, interval), FrameGenerationPlan::Warmup);
        let mut real = 0;
        pacer.presented_real(real);
        for _ in 0..1_000 {
            // Rendering finishes 5 ms after the last real presentation; the wake is 20 us late.
            let FrameGenerationPlan::Pair { real_ns, .. } = pacer.plan(real + 5_000_000, interval)
            else {
                panic!("steady candidate bypassed");
            };
            real = real_ns + 20_000;
            pacer.presented_real(real);
        }
        assert!(
            real <= 1_000 * (interval + 20_000),
            "real interval grew: {real}"
        );
    }

    #[test]
    fn a_stalled_generated_present_never_holds_the_real_image_for_the_stall_again() {
        let interval = 16_666_667;
        let mut pacer = FrameGenerationPacer::default();
        pacer.presented_real(0);
        // The generated present returned 30 ms late; mirroring it would hold the real to 76 ms.
        let late = interval / 2 + 30_000_000;
        assert_eq!(
            pacer.real_deadline(late, interval),
            Some(interval + interval / 8)
        );
        assert_eq!(
            pacer.real_deadline(interval / 2, interval),
            Some(interval - 1)
        );
    }

    #[test]
    fn reset_drops_the_anchor_but_not_the_backoff() {
        let mut pacer = FrameGenerationPacer::default();
        pacer.presented_real(10);
        pacer.reset();
        assert_eq!(pacer.plan(20, 16), FrameGenerationPlan::Warmup);
        pacer.missed();
        pacer.reset();
        assert!(!pacer.admit());
    }

    #[test]
    fn description_states_the_latency_bound_and_admission_rate() {
        let text = frame_generation_description();
        assert!(text.contains("17 ms"), "{text}");
        assert!(
            text.contains(&format!("{FRAME_GENERATION_MIN_FPS} real FPS")),
            "{text}"
        );
    }
}
