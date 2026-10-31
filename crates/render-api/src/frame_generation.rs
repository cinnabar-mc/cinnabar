//! Presentation-only admission and midpoint deadlines; callers supply monotonic nanoseconds.

/// Frame generation is opt-in for every saved profile and device.
pub const DEFAULT_FRAME_GENERATION: bool = false;
/// Interpolation is bypassed below this real-frame rate.
pub const FRAME_GENERATION_MIN_FPS: u64 = 45;
/// A late candidate never extends presentation by more than this fraction of a real interval.
const LATE_DIVISOR: u64 = 8;
/// Shared player-facing latency description, without a hardware measurement claim.
pub const FRAME_GENERATION_DESCRIPTION: &str = "Adds an interpolated world frame between real frames. Adds input and presentation latency while buffering frames. Requires a steady base frame rate. Automatically pauses for captures, camera cuts and unsupported effects. Menus and the HUD stay at native resolution.";

/// Conditions that make an extra presentation unsafe or unhelpful.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameGenerationBypass {
    Disabled,
    Preparing,
    Unsupported,
    Capture,
    Discontinuity,
    LowFrameRate,
    DisplayCapacity,
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
    pub interval_ns: u64,
    /// Synchronized output needs room for two refreshes per real interval.
    pub synchronized_refresh_ns: Option<u64>,
}

impl FrameGenerationAdmission {
    /// Rejects a candidate before encoding interpolation or delaying a real frame.
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
        if self.interval_ns == 0
            || self.interval_ns > 1_000_000_000_u64.div_ceil(FRAME_GENERATION_MIN_FPS)
        {
            return Some(LowFrameRate);
        }
        if self.synchronized_refresh_ns.is_some_and(|refresh| {
            refresh == 0 || self.interval_ns.saturating_add(2) < refresh.saturating_mul(2)
        }) {
            return Some(DisplayCapacity);
        }
        None
    }
}

/// Deadlines for a ready image pair, or a first real image that seeds the delayed timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameGenerationPlan {
    Warmup { real_ns: u64 },
    Pair { generated_ns: u64, real_ns: u64 },
    Bypass(FrameGenerationBypass),
}

/// Anchors an interpolated image to the last successful real presentation.
#[derive(Default, Debug)]
pub struct FrameGenerationPacer {
    real_ns: Option<u64>,
}

impl FrameGenerationPacer {
    /// A bypass, surface change or failed present must never bridge an older pair.
    pub fn reset(&mut self) {
        self.real_ns = None;
    }

    /// Records only a real presentation that completed while interpolation was admitted.
    pub fn presented_real(&mut self, at_ns: u64) {
        self.real_ns = Some(at_ns);
    }

    /// Computes an exact midpoint in the submission timeline without reading the clock.
    pub fn plan(&mut self, ready_ns: u64, interval_ns: u64) -> FrameGenerationPlan {
        let half = interval_ns / 2;
        let Some(previous) = self.real_ns else {
            return FrameGenerationPlan::Warmup {
                real_ns: ready_ns.saturating_add(half),
            };
        };
        let midpoint = previous.saturating_add(half);
        if ready_ns < previous || ready_ns > midpoint.saturating_add(interval_ns / LATE_DIVISOR) {
            self.reset();
            return FrameGenerationPlan::Bypass(FrameGenerationBypass::MissedDeadline);
        }
        let generated_ns = ready_ns.max(midpoint);
        FrameGenerationPlan::Pair {
            generated_ns,
            real_ns: generated_ns.saturating_add(generated_ns.saturating_sub(previous)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Supplies an otherwise eligible high-refresh real frame.
    fn eligible() -> FrameGenerationAdmission {
        FrameGenerationAdmission {
            enabled: true,
            prepared: true,
            compatible: true,
            capture: false,
            continuous: true,
            interval_ns: 16_000_000,
            synchronized_refresh_ns: Some(8_000_000),
        }
    }
    #[test]
    fn runtime_bypasses_preserve_ordinary_real_frame_admission() {
        assert_eq!(eligible().bypass(), None);
        for reason in [
            FrameGenerationBypass::Disabled,
            FrameGenerationBypass::Preparing,
            FrameGenerationBypass::Unsupported,
            FrameGenerationBypass::Capture,
            FrameGenerationBypass::Discontinuity,
            FrameGenerationBypass::LowFrameRate,
            FrameGenerationBypass::DisplayCapacity,
        ] {
            let mut facts = eligible();
            match reason {
                FrameGenerationBypass::Disabled => facts.enabled = false,
                FrameGenerationBypass::Preparing => facts.prepared = false,
                FrameGenerationBypass::Unsupported => facts.compatible = false,
                FrameGenerationBypass::Capture => facts.capture = true,
                FrameGenerationBypass::Discontinuity => facts.continuous = false,
                FrameGenerationBypass::LowFrameRate => facts.interval_ns = 30_000_000,
                FrameGenerationBypass::DisplayCapacity => {
                    facts.synchronized_refresh_ns = Some(9_000_000)
                }
                FrameGenerationBypass::MissedDeadline => unreachable!(),
            }
            assert_eq!(facts.bypass(), Some(reason));
        }
        let mut boundary = eligible();
        boundary.interval_ns = 1_000_000_000_u64.div_ceil(FRAME_GENERATION_MIN_FPS);
        boundary.synchronized_refresh_ns = Some(boundary.interval_ns.div_ceil(2));
        assert_eq!(
            boundary.bypass(),
            None,
            "nanosecond rounding must admit the threshold"
        );
        boundary.synchronized_refresh_ns = Some(0);
        assert_eq!(
            boundary.bypass(),
            Some(FrameGenerationBypass::DisplayCapacity)
        );
        let mut unsynchronized = eligible();
        unsynchronized.synchronized_refresh_ns = None;
        assert_eq!(unsynchronized.bypass(), None);
    }
    #[test]
    fn ready_candidates_are_midpoints_and_late_candidates_reset_the_anchor() {
        let mut pacer = FrameGenerationPacer::default();
        assert_eq!(
            pacer.plan(100, 16),
            FrameGenerationPlan::Warmup { real_ns: 108 }
        );
        pacer.presented_real(108);
        assert_eq!(
            pacer.plan(116, 16),
            FrameGenerationPlan::Pair {
                generated_ns: 116,
                real_ns: 124
            }
        );
        assert_eq!(
            pacer.plan(118, 16),
            FrameGenerationPlan::Pair {
                generated_ns: 118,
                real_ns: 128
            }
        );
        assert_eq!(
            pacer.plan(119, 16),
            FrameGenerationPlan::Bypass(FrameGenerationBypass::MissedDeadline)
        );
        assert_eq!(
            pacer.plan(120, 16),
            FrameGenerationPlan::Warmup { real_ns: 128 }
        );
        pacer.presented_real(128);
        pacer.reset();
        assert_eq!(
            pacer.plan(130, 16),
            FrameGenerationPlan::Warmup { real_ns: 138 }
        );
    }
}
