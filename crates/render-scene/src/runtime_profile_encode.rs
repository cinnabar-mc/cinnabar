//! Command-encoding work counted by the GPU API itself. The render thread publishes the
//! device's running totals once per render frame; a slow-frame report shows what was added
//! since the previous frame boundary.

use std::{
    fmt::Write,
    sync::{Mutex, PoisonError},
};

/// Declares [`EncodeCounter`], its `ALL` table and its report names from one list, so a counter
/// cannot be added to one and missed in another.
macro_rules! encode_counters {
    ($($(#[$doc:meta])* $variant:ident => $name:literal,)+) => {
        /// Work the GPU API did to turn recorded commands into backend command buffers. Names
        /// ending in `_us` are microseconds spent, driver calls included; the others are
        /// counts. Backends that do not count report zero; only Vulkan fills the backend half.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(usize)]
        pub enum EncodeCounter {
            $($(#[$doc])* $variant,)+
        }

        impl EncodeCounter {
            /// Every counter, in discriminant order.
            pub const ALL: [Self; [$($name),+].len()] = [$(Self::$variant),+];

            /// The counter's name in report lines.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                }
            }
        }
    };
}

encode_counters! {
    /// Command encoders finished.
    Encoders => "enc",
    /// Finishing command encoders, which holds every phase below except `pool_reset_us`.
    FinishMicros => "finish_us",
    /// Before each pass: merging its resource usage into the encoder's tracker, the barriers
    /// that follow, query resets and discarded-surface fixups.
    PassPrepareMicros => "prep_us",
    /// Replaying recorded passes into the backend, from pass begin to pass end.
    PassReplayMicros => "replay_us",
    /// Backend render passes begun, including the API's own clear passes.
    RenderPasses => "rp",
    /// Beginning backend render passes, inside `replay_us`, including `fb_new_us`.
    RenderPassBeginMicros => "rp_begin_us",
    /// Framebuffers created because none was cached for a pass's attachments.
    FramebuffersCreated => "fb_new",
    /// Creating those framebuffers.
    FramebufferCreateMicros => "fb_new_us",
    /// Texture-to-texture copies encoded.
    TextureCopies => "tex_copy",
    /// Buffer-to-buffer, buffer-to-texture and texture-to-buffer copies encoded, queue writes
    /// included.
    OtherCopies => "copy",
    /// Encoding all copies.
    CopyMicros => "copy_us",
    /// Buffer and texture barriers encoded.
    Barriers => "bar",
    /// Encoding barriers.
    BarrierMicros => "bar_us",
    /// Ending backend command buffers.
    CommandBufferEndMicros => "cb_end_us",
    /// Resetting the command pools of finished encoders for reuse, in the device poll after
    /// submission rather than while finishing.
    PoolResetMicros => "pool_reset_us",
}

const COUNTERS: usize = EncodeCounter::ALL.len();

/// One value per [`EncodeCounter`], in [`EncodeCounter::ALL`] order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodeCounts(pub [u64; COUNTERS]);

impl Default for EncodeCounts {
    fn default() -> Self {
        Self([0; COUNTERS])
    }
}

impl EncodeCounts {
    /// The value of `counter`.
    #[must_use]
    pub fn get(&self, counter: EncodeCounter) -> u64 {
        self.0[counter as usize]
    }

    /// What was added to these totals since `baseline`. Totals that went down, as after the
    /// device is replaced, read as nothing added.
    #[must_use]
    pub fn since(&self, baseline: &Self) -> Self {
        Self(std::array::from_fn(|index| {
            self.0[index].saturating_sub(baseline.0[index])
        }))
    }

    /// Appends ` encode=name:value,...` with the counters that are not zero, or nothing when
    /// all are.
    pub fn append_fields(&self, line: &mut String) {
        let mut separator = " encode=";
        for counter in EncodeCounter::ALL {
            let value = self.get(counter);
            if value != 0 {
                let _ = write!(line, "{separator}{}:{value}", counter.name());
                separator = ",";
            }
        }
    }
}

static TOTALS: Mutex<EncodeCounts> = Mutex::new(EncodeCounts([0; COUNTERS]));

/// Publishes the device's totals since it was created. The render thread calls this once per
/// render frame, after the frame's encoding and submission.
pub fn publish(totals: EncodeCounts) {
    *TOTALS.lock().unwrap_or_else(PoisonError::into_inner) = totals;
}

/// The totals as last published.
#[must_use]
pub fn latest() -> EncodeCounts {
    *TOTALS.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(values: &[(EncodeCounter, u64)]) -> EncodeCounts {
        let mut counts = EncodeCounts::default();
        for &(counter, value) in values {
            counts.0[counter as usize] = value;
        }
        counts
    }

    #[test]
    fn since_reports_what_was_added_and_nothing_after_a_restart() {
        let earlier = counts(&[
            (EncodeCounter::RenderPasses, 10),
            (EncodeCounter::Barriers, 7),
        ]);
        let later = counts(&[
            (EncodeCounter::RenderPasses, 13),
            (EncodeCounter::Barriers, 2),
        ]);
        let added = later.since(&earlier);
        assert_eq!(added.get(EncodeCounter::RenderPasses), 3);
        assert_eq!(
            added.get(EncodeCounter::Barriers),
            0,
            "a replaced device restarts"
        );
    }

    #[test]
    fn report_fields_skip_zero_counters() {
        let mut line = String::new();
        EncodeCounts::default().append_fields(&mut line);
        assert_eq!(line, "", "nothing counted, no field");
        counts(&[
            (EncodeCounter::RenderPasses, 3),
            (EncodeCounter::FramebufferCreateMicros, 4_200),
        ])
        .append_fields(&mut line);
        assert_eq!(line, " encode=rp:3,fb_new_us:4200");
    }

    #[test]
    fn report_names_are_unique() {
        for (index, counter) in EncodeCounter::ALL.into_iter().enumerate() {
            assert_eq!(counter as usize, index);
            for other in &EncodeCounter::ALL[index + 1..] {
                assert_ne!(counter.name(), other.name());
            }
        }
    }
}
