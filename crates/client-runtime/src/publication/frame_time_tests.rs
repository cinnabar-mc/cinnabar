//! Phone publication limits and their reaction to render frames slower than the device's
//! own normal.

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
/// A 120 Hz panel: the hitch threshold is 1.5 display intervals.
const DISPLAY_INTERVAL: Duration = Duration::from_nanos(8_333_333);
const HITCH: Duration = Duration::from_micros(12_500);
/// A phone that steadily renders slower than its panel, for reasons other than uploads.
const TYPICAL: Duration = Duration::from_millis(22);
/// More than the margin over `TYPICAL`.
const SPIKE: Duration = Duration::from_millis(60);
/// Under the hitch threshold.
const FAST: Duration = Duration::from_millis(5);
/// Time between main frames, matching `TYPICAL`.
const FRAME: Duration = Duration::from_millis(22);
/// A dense section: small enough that the byte rate, not the item rate, bounds service.
const PAYLOAD: u64 = 64 * 1024;
const FULL: (usize, u64) = (MOBILE.maximum_frame_items, MOBILE.maximum_frame_bytes);
const FLOOR: (usize, u64) = (PRESSURE.minimum_frame_items, PRESSURE.minimum_frame_bytes);

/// Drives a controller through frames whose render-thread time the test chooses.
struct Frames {
    controller: PublicationController,
    render_frames: u64,
    /// Whether the previous frame admitted more than the pressure floors.
    heavy: bool,
}

impl Frames {
    fn new(config: PublicationServiceConfig) -> Self {
        Self {
            controller: PublicationController::new(config),
            render_frames: 0,
            heavy: false,
        }
    }

    /// A phone that idled at `TYPICAL` long enough to fill the baseline and bank a burst.
    fn warmed() -> Self {
        let mut frames = Self::new(MOBILE);
        frames.idle(PRESSURE.baseline_frames, TYPICAL);
        assert_eq!(frames.baseline(), Some(TYPICAL));
        assert_eq!(frames.caps(), FULL);
        frames
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
        self.heavy = false;
    }

    /// Opens the next frame without a new render frame: the renderer has not finished one.
    fn begin_without_render_frame(&mut self) {
        self.controller
            .finish_frame(PublicationFrameWork::default());
        self.controller.begin_frame(FRAME);
        self.heavy = false;
    }

    /// Runs frames that publish nothing.
    fn idle(&mut self, frames: usize, render: Duration) {
        for _ in 0..frames {
            self.begin(render, FRAME);
        }
    }

    /// Publishes everything the open frame admits; returns its bytes.
    fn publish_open_frame(&mut self) -> u64 {
        let allowance = self.controller.allowance();
        let mut items = 0;
        while let Some(permit) = allowance.try_admit_payload(PAYLOAD) {
            items += 1;
            assert!(permit.retire());
        }
        let bytes = items as u64 * PAYLOAD;
        self.heavy = items > FLOOR.0 || bytes > FLOOR.1;
        bytes
    }

    /// Runs frames of `render` length that publish as much as each admits; returns the
    /// bytes each published.
    fn publish(&mut self, frames: usize, render: Duration) -> Vec<u64> {
        (0..frames)
            .map(|_| {
                self.begin(render, FRAME);
                self.publish_open_frame()
            })
            .collect()
    }

    /// Like [`Self::publish`], on a device whose uploads cost frame time: a render frame
    /// takes `loaded` after a frame that admitted more than the pressure floors, and
    /// `TYPICAL` otherwise.
    fn publish_upload_bound(&mut self, frames: usize, loaded: Duration) -> Vec<u64> {
        (0..frames)
            .map(|_| {
                self.begin(if self.heavy { loaded } else { TYPICAL }, FRAME);
                self.publish_open_frame()
            })
            .collect()
    }

    fn caps(&self) -> (usize, u64) {
        let budget = self.controller.budget();
        (budget.max_per_frame, budget.max_bytes_per_frame)
    }

    fn baseline(&self) -> Option<Duration> {
        self.controller.diagnostics().render_frame_baseline
    }

    fn decreases(&self) -> u64 {
        self.controller.diagnostics().multiplicative_decreases
    }
}

/// Bytes the byte rate grants over `frames` frames.
fn granted(rate: u64, frames: usize) -> u64 {
    (u128::from(rate) * FRAME.as_nanos() * frames as u128 / 1_000_000_000) as u64
}

/// Whether published bytes match a rate to within payload rounding.
fn near(bytes: u64, expected: u64) -> bool {
    bytes.abs_diff(expected) <= 2 * PAYLOAD
}

/// With a full burst banked, a phone frame still carries at most its item and byte caps.
#[test]
fn the_phone_configuration_caps_each_frame_of_a_banked_burst() {
    let mut frames = Frames::new(MOBILE);
    frames.idle(PRESSURE.baseline_warmup_frames, FAST);
    frames.begin(FAST, Duration::from_secs(10));
    assert_eq!(frames.caps(), FULL);
    assert_eq!(
        frames.controller.accrued_bytes(),
        MOBILE.maximum_burst_bytes
    );

    let allowance = frames.controller.allowance();
    let small = std::iter::from_fn(|| allowance.try_admit_payload(1))
        .map(|permit| assert!(permit.retire()))
        .count();
    assert_eq!(small, MOBILE.maximum_frame_items);

    for bytes in frames.publish(3, FAST) {
        assert_eq!(bytes, MOBILE.maximum_frame_bytes);
    }
}

/// A phone that steadily renders at 22 ms on a 120 Hz panel is over the hitch threshold
/// every frame, yet keeps full service: that is its normal, not upload pressure.
#[test]
fn steady_frames_slower_than_the_display_keep_full_service() {
    assert!(TYPICAL > HITCH);
    let mut frames = Frames::warmed();

    // Spend the banked burst, then measure the sustained rate.
    frames.publish(50, TYPICAL);
    let published: u64 = frames.publish(500, TYPICAL).iter().sum();

    assert!(near(
        published,
        granted(MOBILE.target_bytes_per_second, 500)
    ));
    assert_eq!(frames.caps().0, FULL.0);
    assert_eq!(frames.decreases(), 0);
    assert_eq!(frames.baseline(), Some(TYPICAL));
}

/// Until enough lightly loaded frames have shown the device's normal, no frame is slow and
/// publication is held to the floors so that the arriving frames can show it.
#[test]
fn no_frame_is_slow_before_the_device_has_a_baseline() {
    let mut frames = Frames::new(MOBILE);
    frames.idle(PRESSURE.baseline_warmup_frames - 1, SPIKE);
    assert_eq!(frames.baseline(), None);
    assert_eq!(frames.caps(), FLOOR);
    assert_eq!(frames.decreases(), 0);

    frames.idle(1, SPIKE);
    assert_eq!(frames.baseline(), Some(SPIKE));
    assert_eq!(frames.caps(), FULL);
    assert_eq!(frames.decreases(), 0);
}

/// Streaming that starts with the controller's first frame cannot keep the baseline from
/// forming: publication stays at the floors until it has, and upload-driven slow frames
/// then back off as usual.
#[test]
fn streaming_from_the_first_frame_still_learns_a_baseline_and_backs_off() {
    let mut frames = Frames::new(MOBILE);
    let warmup = PRESSURE.baseline_warmup_frames;

    let learning = frames.publish_upload_bound(warmup - 1, Duration::from_millis(45));
    assert!(learning.iter().all(|bytes| (1..=FLOOR.1).contains(bytes)));
    assert_eq!(frames.baseline(), None);

    frames.publish_upload_bound(warmup, Duration::from_millis(45));
    assert_eq!(frames.baseline(), Some(TYPICAL));
    assert!(
        frames.decreases() >= 1,
        "slow frames after warm-up went unnoticed"
    );
}

/// A renderer that finishes a frame only every other main frame still gets a baseline:
/// the hold bridges the frames between its samples, so no heavy frame slips in and keeps
/// them ineligible.
#[test]
fn a_renderer_slower_than_the_main_loop_still_learns_a_baseline() {
    let mut frames = Frames::new(MOBILE);
    for _ in 0..PRESSURE.baseline_warmup_frames - 1 {
        frames.begin(TYPICAL, FRAME);
        assert!(frames.publish_open_frame() <= FLOOR.1);
        frames.begin_without_render_frame();
        assert!(frames.publish_open_frame() <= FLOOR.1);
    }
    assert_eq!(frames.baseline(), None);

    frames.begin(TYPICAL, FRAME);
    assert_eq!(frames.baseline(), Some(TYPICAL));
    assert_eq!(frames.caps(), FULL);
}

/// Sections larger than the floors publish one per frame during warm-up. They cannot keep
/// the baseline from forming and so extend the hold without end.
#[test]
fn payloads_larger_than_the_floors_cannot_extend_the_warm_up_hold() {
    let mut frames = Frames::new(MOBILE);
    let section = FLOOR.1 + PAYLOAD;
    for frame in 0..PRESSURE.baseline_warmup_frames {
        assert_eq!(frames.baseline(), None, "frame {frame}");
        frames.begin(TYPICAL, FRAME);
        let permit = frames.controller.allowance().try_admit_payload(section);
        assert!(
            permit.is_some_and(|permit| permit.retire()),
            "frame {frame}"
        );
    }
    assert_eq!(frames.baseline(), Some(TYPICAL));
    assert_eq!(frames.caps(), FULL);
}

/// A renderer that stops reporting during warm-up ends the hold after a bounded number of
/// frames, so publication is not left at the floors waiting for a baseline.
#[test]
fn the_warm_up_hold_ends_when_render_frames_stop_arriving() {
    let mut frames = Frames::new(MOBILE);
    frames.publish(5, TYPICAL);
    assert_eq!(frames.baseline(), None);

    let held = (0..100)
        .take_while(|_| {
            frames.begin_without_render_frame();
            frames.caps() == FLOOR
        })
        .count();
    assert!((1..=10).contains(&held), "held for {held} frames");
    assert_eq!(frames.caps(), FULL);
    assert_eq!(frames.baseline(), None);
}

/// Without render-frame samples there is nothing to learn from, so publication is not held
/// to the floors while waiting for a baseline that cannot form.
#[test]
fn publication_is_not_held_back_when_no_render_frames_arrive() {
    let mut controller = PublicationController::new(MOBILE);
    for _ in 0..3 {
        controller.begin_frame(Duration::from_secs(10));
        let budget = controller.budget();
        assert_eq!((budget.max_per_frame, budget.max_bytes_per_frame), FULL);
    }
}

/// Frames well over the device's normal are pressure: each run of them halves the caps
/// down to their floors. Each streak of healthy frames then restores one step, as before.
#[test]
fn a_spike_over_the_baseline_halves_the_caps_and_healthy_frames_restore_them() {
    let mut frames = Frames::warmed();

    for _ in 1..PRESSURE.consecutive_slow_frames {
        frames.idle(1, SPIKE);
        assert_eq!(
            frames.caps(),
            FULL,
            "a short run of slow frames is not pressure"
        );
    }
    frames.idle(1, SPIKE);
    assert_eq!(frames.caps(), (FULL.0 / 2, FULL.1 / 2));
    assert_eq!(frames.decreases(), 1);

    frames.idle(10 * PRESSURE.consecutive_slow_frames as usize, SPIKE);
    assert_eq!(frames.caps(), FLOOR);
    assert_eq!(frames.decreases(), 2);

    for _ in 1..RECOVERY_STREAK_FRAMES {
        frames.idle(1, TYPICAL);
        assert_eq!(
            frames.caps(),
            FLOOR,
            "recovery waits for a full healthy streak"
        );
    }
    frames.idle(1, TYPICAL);
    assert_eq!(
        frames.caps(),
        (FLOOR.0 * 2, FLOOR.1 + PRESSURE.minimum_frame_bytes)
    );
    assert_eq!(frames.controller.diagnostics().additive_increases, 1);

    frames.idle(3 * RECOVERY_STREAK_FRAMES as usize, TYPICAL);
    assert_eq!(frames.caps(), FULL);
    assert_eq!(frames.controller.diagnostics().additive_increases, 3);
    assert_eq!(frames.baseline(), Some(TYPICAL));
}

/// The byte rate follows the caps: halved to its minimum under pressure, then restored.
#[test]
fn a_spike_reduces_the_byte_rate_and_healthy_frames_recover_it() {
    let mut frames = Frames::warmed();
    frames.publish(50, TYPICAL);
    let window = 40;
    let rate = |frames: &mut Frames, render| frames.publish(window, render).iter().sum::<u64>();

    assert!(near(
        rate(&mut frames, TYPICAL),
        granted(MOBILE.target_bytes_per_second, window)
    ));

    // The first slow window still spends credit granted at the faster rates.
    rate(&mut frames, SPIKE);
    assert!(near(
        rate(&mut frames, SPIKE),
        granted(MOBILE.minimum_bytes_per_second, window)
    ));

    frames.publish(4 * RECOVERY_STREAK_FRAMES as usize, TYPICAL);
    assert!(near(
        rate(&mut frames, TYPICAL),
        granted(MOBILE.target_bytes_per_second, window)
    ));
}

/// When uploads themselves slow the frames, service keeps backing off for as long as it
/// lasts: frames after heavy publication never enter the baseline, so it cannot drift up
/// and accept the slowdown.
#[test]
fn a_sustained_upload_driven_slowdown_keeps_backing_off() {
    let mut frames = Frames::warmed();
    let loaded = Duration::from_millis(45);

    frames.publish_upload_bound(500, loaded);
    assert!(frames.decreases() >= 2);
    let before = frames.decreases();

    let run = 2_000;
    let published: u64 = frames.publish_upload_bound(run, loaded).iter().sum();

    assert!(frames.decreases() >= before + 4, "back-off stopped");
    assert_eq!(
        frames.baseline(),
        Some(TYPICAL),
        "the baseline absorbed the slowdown"
    );
    assert!(published < granted(MOBILE.target_bytes_per_second, run) * 3 / 4);
    assert!(published + 2 * PAYLOAD >= granted(MOBILE.minimum_bytes_per_second, run));
}

/// A slowdown that back-off does not cure is not caused by uploads. Once publication is at
/// the floors its frames feed the baseline, which learns the new normal, and full service
/// returns and stays.
#[test]
fn a_slowdown_that_back_off_does_not_cure_becomes_the_new_normal() {
    let mut frames = Frames::warmed();
    let heavier_scene = Duration::from_millis(45);

    frames.publish(PRESSURE.baseline_frames, heavier_scene);
    assert_eq!(frames.decreases(), 2);
    assert_eq!(frames.baseline(), Some(heavier_scene));

    frames.publish(4 * RECOVERY_STREAK_FRAMES as usize, heavier_scene);
    assert_eq!(frames.caps().0, FULL.0);
    let published: u64 = frames.publish(500, heavier_scene).iter().sum();
    assert!(near(
        published,
        granted(MOBILE.target_bytes_per_second, 500)
    ));
    assert_eq!(frames.decreases(), 2);
}

/// However long uploads keep frames slow, every frame keeps publishing geometry and
/// removals, at no less than the minimum service.
#[test]
fn publication_keeps_progressing_under_unending_frame_time_pressure() {
    let mut frames = Frames::warmed();
    // Spend the banked burst, so payloads leave room in the shared operation cap.
    frames.publish(50, TYPICAL);
    let mut published = 0;
    let run = 1_500;
    for _ in 0..run {
        let bytes = frames.publish_upload_bound(1, SPIKE)[0];
        assert!(bytes > 0, "a frame under pressure published nothing");
        published += bytes;
        let budget = frames.controller.budget();
        assert!(budget.max_per_frame >= FLOOR.0);
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
    assert!(frames.decreases() >= 2);
    assert!(published + 2 * PAYLOAD >= granted(MOBILE.minimum_bytes_per_second, run));
}

/// A payload above the pressure-reduced frame cap still publishes, within a few frames.
#[test]
fn a_payload_above_the_reduced_frame_cap_still_publishes_under_pressure() {
    let mut frames = Frames::warmed();
    frames.publish(2 * PRESSURE.consecutive_slow_frames as usize, SPIKE);
    frames.publish(40, SPIKE);
    assert_eq!(frames.decreases(), 2);
    let oversized = MOBILE.maximum_frame_bytes + 1;
    assert!(oversized > frames.caps().1);

    let waited = (0..60)
        .position(|_| {
            frames.begin(SPIKE, FRAME);
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
    let mut frames = Frames::warmed();
    for _ in 0..200 {
        frames.publish(PRESSURE.consecutive_slow_frames as usize - 1, SPIKE);
        frames.publish(1, TYPICAL);
    }
    assert_eq!(frames.caps().0, FULL.0);
    assert_eq!(frames.decreases(), 0);
}

/// One slow render frame reported on many main frames counts once.
#[test]
fn a_repeated_render_frame_sample_counts_once() {
    let mut frames = Frames::warmed();
    let sample = RenderFrameSample {
        sequence: frames.render_frames + 1,
        elapsed: SPIKE,
        hitch: HITCH,
    };
    for _ in 0..10 * PRESSURE.consecutive_slow_frames {
        frames.controller.observe_render_frame(sample);
        frames.controller.begin_frame(FRAME);
    }
    assert_eq!(frames.decreases(), 0);
}

/// Desktop limits ignore render-frame time, so a slow frame cannot slow streaming there.
#[test]
fn the_desktop_configuration_ignores_slow_render_frames() {
    let desktop = PublicationServiceConfig::DESKTOP;
    let mut frames = Frames::new(desktop);
    frames.idle(300, FAST);
    frames.idle(120, SPIKE);
    assert_eq!(
        frames.caps(),
        (desktop.maximum_frame_items, desktop.maximum_frame_bytes)
    );
    assert_eq!(frames.decreases(), 0);
    assert_eq!(frames.baseline(), None);
}

/// The frame system feeds the profiler's render-frame time to the controller.
#[test]
fn the_publication_frame_reads_render_frame_time_from_the_profiler() {
    let profiler = RuntimeStageProfiler::for_gameplay(false, None);
    profiler.set_frame_interval(DISPLAY_INTERVAL);
    let mut app = App::new();
    app.init_resource::<Time<Real>>()
        .init_resource::<ChunkUploadBudget>()
        .init_resource::<ChunkRenderQueue>()
        .insert_resource(profiler.clone())
        .insert_resource(PublicationController::new(MOBILE));
    configure_publication_frame(&mut app);

    for _ in 0..PRESSURE.baseline_warmup_frames {
        profiler.record_background_sample(RuntimeStage::RenderFrame, TYPICAL);
        app.update();
    }
    let diagnostics = app
        .world()
        .resource::<PublicationController>()
        .diagnostics();
    assert_eq!(diagnostics.render_frame_baseline, Some(TYPICAL));
    assert_eq!(diagnostics.multiplicative_decreases, 0);

    for _ in 0..PRESSURE.consecutive_slow_frames {
        profiler.record_background_sample(RuntimeStage::RenderFrame, SPIKE);
        app.update();
    }
    let diagnostics = app
        .world()
        .resource::<PublicationController>()
        .diagnostics();
    assert_eq!(diagnostics.multiplicative_decreases, 1);
}
