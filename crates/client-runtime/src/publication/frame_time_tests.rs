//! Phone publication limits and their reaction to slow render frames.

use std::time::Duration;

use bevy::{
    prelude::{App, Time},
    time::Real,
};
use render_api::{FrameTimePressureConfig, PublicationServiceConfig};
use render_scene::{
    ChunkRenderQueue, ChunkUploadBudget, RenderFrameSample, RuntimeStage, RuntimeStageProfiler,
};

use crate::publication::{
    PublicationController, PublicationFrameWork, RECOVERY_STREAK_FRAMES,
    configure_publication_frame,
};

const MOBILE: PublicationServiceConfig = PublicationServiceConfig::MOBILE;
const PRESSURE: FrameTimePressureConfig = MOBILE
    .frame_time_pressure
    .expect("the phone configuration reacts to frame time");
/// Sixty frames of this length last one second, to within 20 ns.
const FRAME: Duration = Duration::from_nanos(16_666_667);
const HITCH: Duration = Duration::from_millis(25);
const SLOW: Duration = Duration::from_millis(40);
const HEALTHY: Duration = Duration::from_millis(8);
/// A dense section: small enough that the byte rate, not the item rate, bounds service.
const PAYLOAD: u64 = 64 * 1024;

/// Drives a controller through frames whose render-thread time the test chooses.
struct Frames {
    controller: PublicationController,
    render_frames: u64,
}

impl Frames {
    fn new(config: PublicationServiceConfig) -> Self {
        Self {
            controller: PublicationController::new(config),
            render_frames: 0,
        }
    }

    /// Reports one new render frame of `render` length, then opens the next frame.
    fn begin(&mut self, render: Duration, elapsed: Duration) {
        self.render_frames += 1;
        self.controller.observe_render_frame(RenderFrameSample {
            sequence: self.render_frames,
            elapsed: render,
            hitch: HITCH,
        });
        self.controller
            .finish_frame(PublicationFrameWork::default());
        self.controller.begin_frame(elapsed);
    }

    /// Runs `frames` frames, publishing as much as each admits; returns bytes per frame.
    fn publish(&mut self, frames: usize, render: Duration) -> Vec<u64> {
        (0..frames)
            .map(|_| {
                self.begin(render, FRAME);
                let allowance = self.controller.allowance();
                let mut bytes = 0;
                while let Some(permit) = allowance.try_admit_payload(PAYLOAD) {
                    bytes += PAYLOAD;
                    assert!(permit.retire());
                }
                bytes
            })
            .collect()
    }

    fn caps(&self) -> (usize, u64) {
        let budget = self.controller.budget();
        (budget.max_per_frame, budget.max_bytes_per_frame)
    }
}

/// With a full burst banked, a phone frame still carries at most its item and byte caps.
#[test]
fn the_phone_configuration_caps_each_frame_of_a_banked_burst() {
    let mut frames = Frames::new(MOBILE);
    frames.begin(HEALTHY, Duration::from_secs(10));
    assert_eq!(
        frames.caps(),
        (MOBILE.maximum_frame_items, MOBILE.maximum_frame_bytes)
    );
    assert_eq!(
        frames.controller.accrued_bytes(),
        MOBILE.maximum_burst_bytes
    );

    let allowance = frames.controller.allowance();
    let small = std::iter::from_fn(|| allowance.try_admit_payload(1))
        .map(|permit| assert!(permit.retire()))
        .count();
    assert_eq!(small, MOBILE.maximum_frame_items);

    for bytes in frames.publish(3, HEALTHY) {
        assert_eq!(bytes, MOBILE.maximum_frame_bytes);
    }
}

/// Every run of slow render frames halves the caps down to their floors, and each streak of
/// healthy frames restores one step until the configured limits return.
#[test]
fn sustained_slow_render_frames_halve_the_frame_caps_and_healthy_frames_restore_them() {
    let mut frames = Frames::new(MOBILE);
    // Bank a full burst so the published byte cap is the frame limit, not the credit.
    frames.begin(HEALTHY, Duration::from_secs(10));
    let full = (MOBILE.maximum_frame_items, MOBILE.maximum_frame_bytes);

    for _ in 1..PRESSURE.consecutive_slow_frames {
        frames.begin(SLOW, FRAME);
        assert_eq!(
            frames.caps(),
            full,
            "a short run of slow frames is not pressure"
        );
    }
    frames.begin(SLOW, FRAME);
    assert_eq!(frames.caps(), (full.0 / 2, full.1 / 2));
    assert_eq!(frames.controller.diagnostics().multiplicative_decreases, 1);

    for _ in 0..10 * PRESSURE.consecutive_slow_frames {
        frames.begin(SLOW, FRAME);
    }
    let floor = (PRESSURE.minimum_frame_items, PRESSURE.minimum_frame_bytes);
    assert_eq!(frames.caps(), floor);
    assert_eq!(frames.controller.diagnostics().multiplicative_decreases, 2);

    for _ in 1..RECOVERY_STREAK_FRAMES {
        frames.begin(HEALTHY, FRAME);
        assert_eq!(
            frames.caps(),
            floor,
            "recovery waits for a full healthy streak"
        );
    }
    frames.begin(HEALTHY, FRAME);
    assert_eq!(
        frames.caps(),
        (floor.0 * 2, floor.1 + PRESSURE.minimum_frame_bytes)
    );
    assert_eq!(frames.controller.diagnostics().additive_increases, 1);

    for _ in 0..3 * RECOVERY_STREAK_FRAMES {
        frames.begin(HEALTHY, FRAME);
    }
    assert_eq!(frames.caps(), full);
    assert_eq!(frames.controller.diagnostics().additive_increases, 3);
}

/// The byte rate follows the caps: halved to its minimum under pressure, then restored.
#[test]
fn sustained_slow_render_frames_reduce_the_byte_rate_and_healthy_frames_recover_it() {
    let mut frames = Frames::new(MOBILE);
    let second = |frames: &mut Frames, render| frames.publish(60, render).iter().sum::<u64>();
    let near = |bytes: u64, rate: u64| bytes.abs_diff(rate) <= 2 * PAYLOAD;

    assert!(near(
        second(&mut frames, HEALTHY),
        MOBILE.target_bytes_per_second
    ));

    // The first slow second still spends credit banked at the faster rates.
    second(&mut frames, SLOW);
    assert!(near(
        second(&mut frames, SLOW),
        MOBILE.minimum_bytes_per_second
    ));

    for _ in 0..4 * RECOVERY_STREAK_FRAMES / 60 {
        second(&mut frames, HEALTHY);
    }
    assert!(near(
        second(&mut frames, HEALTHY),
        MOBILE.target_bytes_per_second
    ));
}

/// However long render frames stay slow, every frame keeps publishing geometry and
/// removals at no less than the minimum service.
#[test]
fn publication_keeps_progressing_under_unending_frame_time_pressure() {
    let mut frames = Frames::new(MOBILE);
    frames.publish(120, SLOW);

    let mut published = 0;
    for _ in 0..600 {
        let bytes = frames.publish(1, SLOW)[0];
        assert!(bytes > 0, "a frame under pressure published nothing");
        published += bytes;
        let budget = frames.controller.budget();
        assert!(budget.max_per_frame >= PRESSURE.minimum_frame_items);
        assert_eq!(
            budget.max_zero_byte_operations_per_frame,
            MOBILE.maximum_zero_byte_operations_per_frame
        );
        assert!(
            frames
                .controller
                .allowance()
                .try_admit_zero_byte()
                .is_some_and(|permit| permit.retire())
        );
    }
    assert!(published + 2 * PAYLOAD >= 10 * MOBILE.minimum_bytes_per_second);
    assert_eq!(frames.controller.diagnostics().multiplicative_decreases, 2);
}

/// A payload above the pressure-reduced frame cap still publishes, within a few frames.
#[test]
fn a_payload_above_the_reduced_frame_cap_still_publishes_under_pressure() {
    let mut frames = Frames::new(MOBILE);
    frames.publish(120, SLOW);
    let oversized = MOBILE.maximum_frame_bytes + 1;
    assert!(oversized > frames.caps().1);

    let waited = (0..60)
        .position(|_| {
            frames.begin(SLOW, FRAME);
            frames
                .controller
                .allowance()
                .try_admit_payload(oversized)
                .is_some_and(|permit| permit.retire())
        })
        .expect("the oversized payload was never admitted");
    assert!(waited <= 8, "waited {waited} frames");
}

/// Isolated slow frames are not pressure: nothing is reduced.
#[test]
fn isolated_slow_render_frames_do_not_reduce_the_service() {
    let mut frames = Frames::new(MOBILE);
    frames.begin(HEALTHY, Duration::from_secs(10));
    let full = frames.caps();
    for _ in 0..200 {
        for _ in 1..PRESSURE.consecutive_slow_frames {
            frames.begin(SLOW, FRAME);
        }
        frames.begin(HEALTHY, FRAME);
    }
    assert_eq!(frames.caps(), full);
    assert_eq!(frames.controller.diagnostics().multiplicative_decreases, 0);
}

/// One slow render frame reported on many main frames counts once.
#[test]
fn a_repeated_render_frame_sample_counts_once() {
    let mut controller = PublicationController::new(MOBILE);
    for _ in 0..10 * PRESSURE.consecutive_slow_frames {
        controller.observe_render_frame(RenderFrameSample {
            sequence: 1,
            elapsed: SLOW,
            hitch: HITCH,
        });
        controller.begin_frame(FRAME);
    }
    assert_eq!(controller.diagnostics().multiplicative_decreases, 0);
}

/// Desktop limits ignore render-frame time, so a slow frame cannot slow streaming there.
#[test]
fn the_desktop_configuration_ignores_slow_render_frames() {
    let desktop = PublicationServiceConfig::DESKTOP;
    let mut frames = Frames::new(desktop);
    frames.begin(HEALTHY, Duration::from_secs(10));
    for _ in 0..120 {
        frames.begin(SLOW, FRAME);
    }
    assert_eq!(
        frames.caps(),
        (desktop.maximum_frame_items, desktop.maximum_frame_bytes)
    );
    assert_eq!(frames.controller.diagnostics().multiplicative_decreases, 0);
}

/// The frame system feeds the profiler's render-frame time to the controller.
#[test]
fn the_publication_frame_reads_render_frame_time_from_the_profiler() {
    let profiler = RuntimeStageProfiler::for_gameplay(false, None);
    profiler.set_frame_interval(FRAME);
    let mut app = App::new();
    app.init_resource::<Time<Real>>()
        .init_resource::<ChunkUploadBudget>()
        .init_resource::<ChunkRenderQueue>()
        .insert_resource(profiler.clone())
        .insert_resource(PublicationController::new(MOBILE));
    configure_publication_frame(&mut app);

    app.update();
    for _ in 0..PRESSURE.consecutive_slow_frames {
        profiler.record_background_sample(RuntimeStage::RenderFrame, 4 * FRAME);
        app.update();
    }

    let controller = app.world().resource::<PublicationController>();
    assert_eq!(controller.diagnostics().multiplicative_decreases, 1);
}
