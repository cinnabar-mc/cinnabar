use std::time::Duration;

use bevy::{
    prelude::{App, IntoScheduleConfigs, Res, ResMut, Resource, Time, Update},
    time::Real,
};
use client_state::RuntimeFrameSet;
use render_api::{FrameTimePressureConfig, PublicationAllowance, PublicationServiceConfig};
use render_scene::{
    ChunkRenderApplySet, ChunkRenderQueue, ChunkUploadBudget, RenderFrameSample,
    RuntimeStageProfiler,
};

mod frame_time;

use frame_time::{FrameTimeFeedback, FrameTimeSignal};

const NANOS_PER_SECOND: u128 = 1_000_000_000;
const RECOVERY_STREAK_FRAMES: u32 = 120;
const RENDER_QUEUE_PRESSURE_ITEMS: usize = 512;

/// Under sustained genuine pressure the per-frame operation caps collapse
/// toward this floor instead of one, so a draining backlog keeps making
/// progress and the pressure state stays escapable.
const MINIMUM_PRESSURE_OPERATIONS_PER_FRAME: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicationFrameWork {
    pub mesh_jobs_dispatched: usize,
    pub mesh_changes_published: usize,
    pub mesh_payloads_published: usize,
    pub mesh_bytes_published: u64,
    pub pending_mesh_jobs: usize,
    pub in_flight_mesh_jobs: usize,
    pub upload_queue_items: usize,
    pub upload_queue_bytes: u64,
    pub allowance_live_permits: usize,
    pub allowance_live_payload_bytes: u64,
    pub stream_pending_mesh_changes: usize,
    pub cohort_expected: usize,
    pub cohort_loaded: usize,
    pub resident_meshes: usize,
    pub cave_visible_meshes: usize,
    pub frustum_visible_meshes: usize,
    pub submitted_meshes: usize,
    pub gpu_completed_meshes: usize,
}

impl PublicationFrameWork {
    #[must_use]
    /// Returns an empty frame observation with no publication pressure.
    pub const fn healthy() -> Self {
        Self {
            mesh_jobs_dispatched: 0,
            mesh_changes_published: 0,
            mesh_payloads_published: 0,
            mesh_bytes_published: 0,
            pending_mesh_jobs: 0,
            in_flight_mesh_jobs: 0,
            upload_queue_items: 0,
            upload_queue_bytes: 0,
            allowance_live_permits: 0,
            allowance_live_payload_bytes: 0,
            stream_pending_mesh_changes: 0,
            cohort_expected: 0,
            cohort_loaded: 0,
            resident_meshes: 0,
            cave_visible_meshes: 0,
            frustum_visible_meshes: 0,
            submitted_meshes: 0,
            gpu_completed_meshes: 0,
        }
    }
}

impl Default for PublicationFrameWork {
    fn default() -> Self {
        Self::healthy()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PublicationDiagnostics {
    pub frame_sequence: u64,
    pub observed_frame_time: Duration,
    pub budget: ChunkUploadBudget,
    pub under_target_streak: u32,
    pub multiplicative_decreases: u64,
    pub additive_increases: u64,
    /// This device's typical render-frame time, once frame-time feedback has measured it.
    pub render_frame_baseline: Option<Duration>,
    pub last_work: PublicationFrameWork,
}

#[derive(Resource, Debug, Clone)]
pub struct PublicationController {
    config: PublicationServiceConfig,
    item_rate_per_second: u32,
    byte_rate_per_second: u64,
    zero_byte_operations_per_frame: usize,
    item_operations_per_frame: usize,
    frame_byte_limit: u64,
    allowance: PublicationAllowance,
    item_numerator_remainder: u128,
    byte_numerator_remainder: u128,
    pressure_active: bool,
    previous_upload_queue_items: usize,
    /// Slow-frame detection, present when the configuration reacts to frame time.
    frame_time: Option<FrameTimeFeedback>,
    /// The frame whose payload admissions frame-time feedback has already been told.
    admissions_noted_for: u64,
    diagnostics: PublicationDiagnostics,
}

impl Default for PublicationController {
    fn default() -> Self {
        Self::new(PublicationServiceConfig::DEFAULT)
    }
}

impl PublicationController {
    #[must_use]
    /// Creates the shared publication allowance from validated service limits.
    pub fn new(config: PublicationServiceConfig) -> Self {
        let config = config.validated();
        let zero_byte_operations_per_frame = config.maximum_zero_byte_operations_per_frame;
        let item_operations_per_frame = config.maximum_frame_items;
        let budget = frame_budget(
            config,
            item_operations_per_frame,
            0,
            0,
            zero_byte_operations_per_frame,
        );
        Self {
            config,
            item_rate_per_second: config.target_items_per_second,
            byte_rate_per_second: config.target_bytes_per_second,
            zero_byte_operations_per_frame,
            item_operations_per_frame,
            frame_byte_limit: config.maximum_frame_bytes,
            allowance: PublicationAllowance::new(config),
            item_numerator_remainder: 0,
            byte_numerator_remainder: 0,
            pressure_active: false,
            previous_upload_queue_items: 0,
            frame_time: config.frame_time_pressure.map(FrameTimeFeedback::new),
            admissions_noted_for: 0,
            diagnostics: PublicationDiagnostics {
                budget,
                ..Default::default()
            },
        }
    }

    /// Refills the frame allowance using elapsed time and the previous frame's pressure.
    pub fn begin_frame(&mut self, elapsed: Duration) {
        self.note_frame_admissions();
        self.diagnostics.frame_sequence = self.diagnostics.frame_sequence.saturating_add(1);
        self.diagnostics.observed_frame_time = elapsed;
        self.update_pressure_state();
        // Frames without a world must not repeat the retired session's backlog.
        self.diagnostics.last_work = PublicationFrameWork::default();

        let (items, item_remainder) = accrue_tokens(
            u128::from(self.item_rate_per_second),
            elapsed,
            self.item_numerator_remainder,
            0,
            self.config.maximum_burst_items as u128,
        );
        let (bytes, byte_remainder) = accrue_tokens(
            u128::from(self.byte_rate_per_second),
            elapsed,
            self.byte_numerator_remainder,
            0,
            u128::from(self.config.maximum_burst_bytes),
        );
        let issued_items = usize::try_from(items)
            .unwrap_or(self.config.maximum_burst_items)
            .min(self.config.maximum_burst_items);
        let issued_bytes = u64::try_from(bytes)
            .unwrap_or(self.config.maximum_burst_bytes)
            .min(self.config.maximum_burst_bytes);
        self.item_numerator_remainder = item_remainder;
        self.byte_numerator_remainder = byte_remainder;
        self.allowance.begin_frame_with_byte_limit(
            self.diagnostics.frame_sequence,
            issued_items,
            issued_bytes,
            self.zero_byte_operations_per_frame,
            self.item_operations_per_frame,
            self.frame_byte_limit,
        );
        self.diagnostics.budget = frame_budget(
            self.config,
            self.item_operations_per_frame,
            self.allowance.frame_remaining_items(),
            self.allowance.frame_remaining_bytes(),
            self.zero_byte_operations_per_frame,
        );
    }

    /// Counts a completed render-thread frame toward the next pressure decision. Call it
    /// before [`Self::begin_frame`]. A sample whose sequence was already counted adds
    /// nothing, so callers may pass the newest sample every frame.
    pub fn observe_render_frame(&mut self, sample: RenderFrameSample) {
        self.note_frame_admissions();
        if let Some(feedback) = &mut self.frame_time {
            feedback.observe(sample);
            self.diagnostics.render_frame_baseline = feedback.typical();
        }
    }

    /// Tells frame-time feedback, once per frame, what the frame that just ended admitted.
    fn note_frame_admissions(&mut self) {
        if self.admissions_noted_for == self.diagnostics.frame_sequence {
            return;
        }
        self.admissions_noted_for = self.diagnostics.frame_sequence;
        if let Some(feedback) = &mut self.frame_time {
            let (items, bytes) = self.allowance.frame_admitted_payloads();
            feedback.note_admissions(items, bytes);
        }
    }

    /// Retains this frame's work for diagnostics and the next pressure decision.
    pub fn finish_frame(&mut self, work: PublicationFrameWork) {
        self.diagnostics.last_work = work;
    }

    /// Replaces the backlog sample with what this frame's render application left queued.
    fn record_applied_backlog(&mut self, items: usize, bytes: u64) {
        self.diagnostics.last_work.upload_queue_items = items;
        self.diagnostics.last_work.upload_queue_bytes = bytes;
    }

    #[must_use]
    /// Returns the current frame's upload and zero-byte operation caps.
    pub const fn budget(&self) -> ChunkUploadBudget {
        self.diagnostics.budget
    }

    #[must_use]
    /// Returns a shared handle to the publication permits.
    pub fn allowance(&self) -> PublicationAllowance {
        self.allowance.clone()
    }

    #[must_use]
    #[cfg(any(test, feature = "test-support"))]
    /// Returns unused item permits for publication regression fixtures.
    pub fn accrued_items(&self) -> usize {
        self.allowance.remaining_items()
    }

    #[must_use]
    #[cfg(any(test, feature = "test-support"))]
    /// Returns unused byte permits for publication regression fixtures.
    pub fn accrued_bytes(&self) -> u64 {
        self.allowance.remaining_bytes()
    }

    #[must_use]
    /// Returns the latest budget and pressure observations.
    pub const fn diagnostics(&self) -> PublicationDiagnostics {
        self.diagnostics
    }

    /// Applies frame-time feedback for a render frame counted since the last decision.
    /// Returns whether that frame was slow, which also holds recovery back.
    ///
    /// Each completed run of consecutive slow frames halves the geometry rates and the
    /// per-frame caps toward their floors, so later frames copy less while the device
    /// catches up. The floors keep nearby sections publishing however long frames stay slow.
    fn apply_frame_time_pressure(&mut self) -> bool {
        let (Some(pressure), Some(feedback)) =
            (self.config.frame_time_pressure, &mut self.frame_time)
        else {
            return false;
        };
        match feedback.take_signal() {
            FrameTimeSignal::Healthy => false,
            FrameTimeSignal::Slow => true,
            FrameTimeSignal::Sustained => {
                self.decrease_for_frame_time(pressure);
                self.diagnostics.under_target_streak = 0;
                true
            }
        }
    }

    /// Halves the geometry rates and frame caps, stopping at the configured floors and
    /// never raising a cap that backlog pressure already took lower.
    fn decrease_for_frame_time(&mut self, pressure: FrameTimePressureConfig) {
        let previous = (
            self.item_rate_per_second,
            self.byte_rate_per_second,
            self.item_operations_per_frame,
            self.frame_byte_limit,
        );
        self.item_rate_per_second =
            (self.item_rate_per_second / 2).max(self.config.minimum_items_per_second);
        self.byte_rate_per_second =
            (self.byte_rate_per_second / 2).max(self.config.minimum_bytes_per_second);
        self.item_operations_per_frame = (self.item_operations_per_frame / 2)
            .max(pressure.minimum_frame_items)
            .min(self.item_operations_per_frame);
        self.frame_byte_limit = (self.frame_byte_limit / 2)
            .max(pressure.minimum_frame_bytes)
            .min(self.frame_byte_limit);
        if previous
            != (
                self.item_rate_per_second,
                self.byte_rate_per_second,
                self.item_operations_per_frame,
                self.frame_byte_limit,
            )
        {
            self.diagnostics.multiplicative_decreases =
                self.diagnostics.multiplicative_decreases.saturating_add(1);
        }
    }

    /// Reduces caps under a stalled backlog or sustained slow render frames, and restores
    /// them one step per streak of healthy frames.
    fn update_pressure_state(&mut self) {
        let slow_render = self.apply_frame_time_pressure();
        let work = self.diagnostics.last_work;
        if work.upload_queue_items >= RENDER_QUEUE_PRESSURE_ITEMS
            || work.upload_queue_bytes >= self.config.maximum_frame_bytes
        {
            self.pressure_active = true;
        } else if work.upload_queue_items < RENDER_QUEUE_PRESSURE_ITEMS / 2
            && work.upload_queue_bytes < self.config.maximum_frame_bytes / 2
        {
            self.pressure_active = false;
        }
        let draining = work.upload_queue_items < self.previous_upload_queue_items;
        self.previous_upload_queue_items = work.upload_queue_items;
        // A backlog that is measurably draining proves the collapsed caps can
        // keep up; halving further would only dig the one-operation trap that
        // once starved live joins. Collapse only while the queue is stuck or
        // still growing.
        let gpu_backlog = self.pressure_active && !draining;
        if gpu_backlog {
            let previous_item_rate = self.item_rate_per_second;
            let previous_byte_rate = self.byte_rate_per_second;
            let previous_zero_cap = self.zero_byte_operations_per_frame;
            let previous_item_cap = self.item_operations_per_frame;
            self.item_rate_per_second = self.config.minimum_items_per_second;
            self.byte_rate_per_second = self.config.minimum_bytes_per_second;
            self.zero_byte_operations_per_frame = halved_operation_cap(
                self.zero_byte_operations_per_frame,
                self.config.maximum_zero_byte_operations_per_frame,
            );
            self.item_operations_per_frame = halved_operation_cap(
                self.item_operations_per_frame,
                self.config.maximum_frame_items,
            );
            if self.item_rate_per_second != previous_item_rate
                || self.byte_rate_per_second != previous_byte_rate
                || self.zero_byte_operations_per_frame != previous_zero_cap
                || self.item_operations_per_frame != previous_item_cap
            {
                self.diagnostics.multiplicative_decreases =
                    self.diagnostics.multiplicative_decreases.saturating_add(1);
            }
            self.diagnostics.under_target_streak = 0;
            return;
        }
        // A slow frame does not count toward recovery; only sustained ones reset it.
        if slow_render {
            return;
        }

        let recovering = self.item_rate_per_second != self.config.target_items_per_second
            || self.byte_rate_per_second != self.config.target_bytes_per_second
            || self.zero_byte_operations_per_frame
                < self.config.maximum_zero_byte_operations_per_frame
            || self.item_operations_per_frame < self.config.maximum_frame_items
            || self.frame_byte_limit < self.config.maximum_frame_bytes;
        if !recovering {
            self.diagnostics.under_target_streak = 0;
            return;
        }
        self.diagnostics.under_target_streak = self
            .diagnostics
            .under_target_streak
            .checked_add(1)
            .unwrap_or(RECOVERY_STREAK_FRAMES)
            .min(RECOVERY_STREAK_FRAMES);
        if self.diagnostics.under_target_streak < RECOVERY_STREAK_FRAMES {
            return;
        }
        self.diagnostics.under_target_streak = 0;
        let rates_recovered = self.item_rate_per_second != self.config.target_items_per_second
            || self.byte_rate_per_second != self.config.target_bytes_per_second;
        // Additive: each step restores one minimum rate and one minimum frame of bytes. A
        // target of twice the minimum therefore returns in a single step.
        self.item_rate_per_second = self
            .item_rate_per_second
            .saturating_add(self.config.minimum_items_per_second)
            .min(self.config.target_items_per_second);
        self.byte_rate_per_second = self
            .byte_rate_per_second
            .saturating_add(self.config.minimum_bytes_per_second)
            .min(self.config.target_bytes_per_second);
        let previous_byte_limit = self.frame_byte_limit;
        self.frame_byte_limit = self
            .frame_byte_limit
            .saturating_add(
                self.config
                    .frame_time_pressure
                    .map_or(self.config.maximum_frame_bytes, |pressure| {
                        pressure.minimum_frame_bytes
                    }),
            )
            .min(self.config.maximum_frame_bytes);
        let previous_zero_cap = self.zero_byte_operations_per_frame;
        self.zero_byte_operations_per_frame = self
            .zero_byte_operations_per_frame
            .saturating_add(1)
            .min(self.config.maximum_zero_byte_operations_per_frame);
        let previous_item_cap = self.item_operations_per_frame;
        self.item_operations_per_frame =
            geometric_item_cap_recovery(previous_item_cap, self.config.maximum_frame_items);
        if rates_recovered
            || self.frame_byte_limit != previous_byte_limit
            || self.zero_byte_operations_per_frame != previous_zero_cap
            || self.item_operations_per_frame != previous_item_cap
        {
            self.diagnostics.additive_increases =
                self.diagnostics.additive_increases.saturating_add(1);
        }
    }
}

/// Refills the allowance before world publication and samples the backlog after render application.
pub fn configure_publication_frame(app: &mut App) {
    app.add_systems(
        Update,
        (
            begin_publication_frame
                .in_set(RuntimeFrameSet::PublicationBudget)
                .before(ChunkRenderApplySet),
            sample_applied_backlog.after(ChunkRenderApplySet),
        ),
    );
}

/// A batch handed off this frame is applied in the same frame, so only the residue is pressure.
fn sample_applied_backlog(
    mut controller: ResMut<PublicationController>,
    queue: Option<Res<ChunkRenderQueue>>,
) {
    if let Some(queue) = queue {
        controller.record_applied_backlog(queue.retained_len(), queue.pending_bytes());
    }
}

/// Refills the runtime allowance and publishes the matching renderer budget, feeding the
/// controller the newest render-thread frame time when the profiler measures one.
pub fn begin_publication_frame(
    time: Res<Time<Real>>,
    profiler: Option<Res<RuntimeStageProfiler>>,
    mut controller: ResMut<PublicationController>,
    mut budget: ResMut<ChunkUploadBudget>,
) {
    if let Some(sample) = profiler.and_then(|profiler| profiler.latest_render_frame()) {
        controller.observe_render_frame(sample);
    }
    controller.begin_frame(time.delta());
    *budget = controller.budget();
}

#[must_use]
/// Formats the latest budget and backlog counters for diagnostics.
pub fn adaptive_publication_diagnostic_line(diagnostics: PublicationDiagnostics) -> String {
    let work = diagnostics.last_work;
    let published_zero_items = work
        .mesh_changes_published
        .saturating_sub(work.mesh_payloads_published);
    format!(
        "ADAPTIVE_PUBLICATION frame={} frame_us={} cap_items={} cap_bytes={} cap_zero={} under_target_streak={} decreases={} increases={} dispatched={} published={} published_payload_items={} published_zero_items={} published_bytes={} pending={} in_flight={} upload_items={} upload_bytes={} live_permits={} live_bytes={} stream_changes={} cohort_loaded={} cohort_expected={} resident={} cave={} frustum={} submitted={} gpu_completed={}",
        diagnostics.frame_sequence,
        diagnostics.observed_frame_time.as_micros(),
        diagnostics.budget.max_per_frame,
        diagnostics.budget.max_bytes_per_frame,
        diagnostics.budget.max_zero_byte_operations_per_frame,
        diagnostics.under_target_streak,
        diagnostics.multiplicative_decreases,
        diagnostics.additive_increases,
        work.mesh_jobs_dispatched,
        work.mesh_changes_published,
        work.mesh_payloads_published,
        published_zero_items,
        work.mesh_bytes_published,
        work.pending_mesh_jobs,
        work.in_flight_mesh_jobs,
        work.upload_queue_items,
        work.upload_queue_bytes,
        work.allowance_live_permits,
        work.allowance_live_payload_bytes,
        work.stream_pending_mesh_changes,
        work.cohort_loaded,
        work.cohort_expected,
        work.resident_meshes,
        work.cave_visible_meshes,
        work.frustum_visible_meshes,
        work.submitted_meshes,
        work.gpu_completed_meshes,
    )
}

/// Bounds renderer work by both available permits and per-frame caps.
fn frame_budget(
    config: PublicationServiceConfig,
    item_operations_per_frame: usize,
    accrued_items: usize,
    accrued_bytes: u64,
    zero_byte_operations_per_frame: usize,
) -> ChunkUploadBudget {
    ChunkUploadBudget::new(
        accrued_items
            .min(item_operations_per_frame)
            .min(config.maximum_frame_items),
        accrued_bytes.min(config.maximum_frame_bytes),
    )
    .with_zero_byte_operations_per_frame(zero_byte_operations_per_frame)
    .with_preparation_time(config.maximum_frame_preparation_time)
}

/// Halves a nonzero operation cap toward the pressure floor without exceeding its maximum.
fn halved_operation_cap(current: usize, configured_maximum: usize) -> usize {
    if current == 0 {
        return 0;
    }
    current
        .saturating_div(2)
        .max(MINIMUM_PRESSURE_OPERATIONS_PER_FRAME)
        .min(configured_maximum)
}

/// Restores a reduced item cap without exceeding its configured maximum.
fn geometric_item_cap_recovery(current: usize, configured_maximum: usize) -> usize {
    if current == 0 || current >= configured_maximum {
        return configured_maximum;
    }
    current
        .saturating_mul(2)
        .max(current.saturating_add(1))
        .min(configured_maximum)
}

/// Converts elapsed time to bounded permits while retaining fractional progress.
fn accrue_tokens(
    rate_per_second: u128,
    elapsed: Duration,
    numerator_remainder: u128,
    accrued: u128,
    ceiling: u128,
) -> (u128, u128) {
    let Some(numerator) = rate_per_second
        .checked_mul(elapsed.as_nanos())
        .and_then(|value| value.checked_add(numerator_remainder))
    else {
        return (ceiling, 0);
    };
    let earned = numerator / NANOS_PER_SECOND;
    let remainder = numerator % NANOS_PER_SECOND;
    let total = accrued.checked_add(earned).unwrap_or(ceiling).min(ceiling);
    (total, if total == ceiling { 0 } else { remainder })
}

#[cfg(test)]
#[path = "publication/pressure_tests.rs"]
mod pressure_tests;

#[cfg(test)]
#[path = "publication/frame_time_tests.rs"]
mod frame_time_tests;
