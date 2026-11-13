//! Publication service limits for each platform.

use std::time::Duration;

const MIB: u64 = 1024 * 1024;

/// Elapsed-time geometry service and per-frame publication work limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicationServiceConfig {
    pub minimum_items_per_second: u32,
    pub minimum_bytes_per_second: u64,
    /// Geometry payload credits; zero-byte work uses its separate frame allowance.
    pub target_items_per_second: u32,
    pub target_bytes_per_second: u64,
    pub maximum_frame_items: usize,
    /// Geometry payload and arena migration bytes one frame may copy to the GPU. A single
    /// larger payload still publishes, alone in its frame.
    pub maximum_frame_bytes: u64,
    /// Largest single payload the service admits, which the render queue must be able to
    /// hold. Larger payloads are never admitted.
    pub maximum_payload_bytes: u64,
    pub maximum_burst_items: usize,
    pub maximum_burst_bytes: u64,
    pub maximum_zero_byte_operations_per_frame: usize,
    pub urgent_zero_byte_reserve: usize,
    /// Render-thread time one frame may spend preparing uploads; the first upload always runs.
    pub maximum_frame_preparation_time: Duration,
    /// Authored texture replacement bytes queued per frame.
    pub maximum_frame_authored_texture_bytes: u64,
    /// GPU-local arena migration copy bytes per frame, inside `maximum_frame_bytes`.
    pub maximum_frame_arena_migration_bytes: u64,
    /// Slows the service while render frames run long; `None` ignores frame time.
    pub frame_time_pressure: Option<FrameTimePressureConfig>,
}

/// When slow render frames count as pressure, and the floors their halving stops at.
///
/// A render frame is slow when it exceeds both the display's hitch threshold and
/// `slow_frame_percent` of this device's typical render-frame time. A device that is
/// steadily slower than its display therefore keeps full service; only frames slow against
/// its own normal reduce it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameTimePressureConfig {
    /// Consecutive slow render frames that halve the service once.
    pub consecutive_slow_frames: u32,
    /// Render frames in the rolling median that estimates the typical render-frame time.
    pub baseline_frames: usize,
    /// Samples the median needs before it is used. Until then no frame counts as slow.
    pub baseline_warmup_frames: usize,
    /// Percentage of the typical render-frame time a slow frame must exceed.
    pub slow_frame_percent: u32,
    /// Lowest per-frame operation cap, so nearby sections keep publishing. Only frames that
    /// admitted no more payloads than this feed the typical frame time, so it measures the
    /// device without the uploads being judged.
    pub minimum_frame_items: usize,
    /// Lowest per-frame payload byte cap, the step each recovery restores, and the byte
    /// counterpart of the admission limit on `minimum_frame_items`.
    pub minimum_frame_bytes: u64,
}

impl PublicationServiceConfig {
    /// Desktop limits, qualified by desktop streaming captures. They ignore frame time: a
    /// slow desktop frame is rarely caused by uploads, and throttling on it would slow
    /// streaming that those captures accept.
    pub const DESKTOP: Self = Self {
        minimum_items_per_second: 4_096,
        minimum_bytes_per_second: 64 * MIB,
        target_items_per_second: 8_192,
        target_bytes_per_second: 128 * MIB,
        maximum_frame_items: 512,
        maximum_frame_bytes: 64 * MIB,
        maximum_payload_bytes: 64 * MIB,
        maximum_burst_items: 8_192,
        maximum_burst_bytes: 128 * MIB,
        maximum_zero_byte_operations_per_frame: 256,
        urgent_zero_byte_reserve: 16,
        // Half the 240 Hz render-CPU p99 budget, leaving the rest of the frame to cull and draw.
        maximum_frame_preparation_time: Duration::from_millis(1),
        maximum_frame_authored_texture_bytes: 8 * MIB,
        maximum_frame_arena_migration_bytes: 16 * MIB,
        frame_time_pressure: None,
    }
    .validated();

    /// Phone and tablet limits. Unified memory moves every uploaded byte about three times
    /// (the CPU write, then the GPU copy's read and write), so a frame at the desktop
    /// ceiling costs about a frame of memory bandwidth. The target rates, the largest
    /// payload, the urgent reserve and the preparation time are desktop's; one frame and one
    /// burst carry an eighth and a quarter of the desktop amounts, and sustained slow render
    /// frames halve the service down to a quarter. These values come from that model and
    /// are not yet tuned on devices.
    pub const MOBILE: Self = Self {
        minimum_items_per_second: 2_048,
        minimum_bytes_per_second: 32 * MIB,
        maximum_frame_items: 128,
        maximum_frame_bytes: 8 * MIB,
        maximum_burst_items: 2_048,
        maximum_burst_bytes: 32 * MIB,
        maximum_zero_byte_operations_per_frame: 64,
        maximum_frame_authored_texture_bytes: 2 * MIB,
        maximum_frame_arena_migration_bytes: 4 * MIB,
        frame_time_pressure: Some(FrameTimePressureConfig {
            consecutive_slow_frames: 3,
            baseline_frames: 240,
            baseline_warmup_frames: 30,
            slow_frame_percent: 150,
            minimum_frame_items: 32,
            minimum_frame_bytes: 2 * MIB,
        }),
        ..Self::DESKTOP
    }
    .validated();

    /// The limits for the platform this client is built for.
    pub const DEFAULT: Self =
        Self::for_platform(cfg!(any(target_os = "android", target_os = "ios")));

    /// Selects the phone and tablet limits or the desktop ones.
    #[must_use]
    pub const fn for_platform(mobile: bool) -> Self {
        if mobile { Self::MOBILE } else { Self::DESKTOP }
    }

    /// Returns the limits after checking that every floor sits at or below its ceiling.
    ///
    /// # Panics
    ///
    /// Panics when a minimum is zero or exceeds its target, or a frame limit exceeds its
    /// burst; a constant using it then fails to compile.
    #[must_use]
    pub const fn validated(self) -> Self {
        assert!(self.minimum_items_per_second > 0);
        assert!(self.minimum_bytes_per_second > 0);
        assert!(self.minimum_items_per_second <= self.target_items_per_second);
        assert!(self.minimum_bytes_per_second <= self.target_bytes_per_second);
        assert!(self.maximum_frame_items <= self.maximum_burst_items);
        assert!(self.maximum_frame_bytes <= self.maximum_burst_bytes);
        assert!(self.maximum_frame_bytes <= self.maximum_payload_bytes);
        assert!(self.maximum_frame_arena_migration_bytes <= self.maximum_frame_bytes);
        if let Some(pressure) = self.frame_time_pressure {
            assert!(pressure.consecutive_slow_frames > 0);
            assert!(pressure.baseline_warmup_frames > 0);
            assert!(pressure.baseline_warmup_frames <= pressure.baseline_frames);
            assert!(pressure.slow_frame_percent >= 100);
            assert!(pressure.minimum_frame_items > 0);
            assert!(pressure.minimum_frame_items <= self.maximum_frame_items);
            assert!(pressure.minimum_frame_bytes > 0);
            assert!(pressure.minimum_frame_bytes <= self.maximum_frame_bytes);
        }
        self
    }
}
