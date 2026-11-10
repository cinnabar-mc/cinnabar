//! Delayed rejection counts admit GPU work; they never authorize hiding terrain.

use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicU64, Ordering},
};

use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;

use super::model::{DECISION_CANDIDATE, DECISION_HIDDEN, STREAM_COUNT};
use super::visibility_admission::{PROBE_INTERVAL, VisibilityAdmission};
use super::work_admission::{
    CALIBRATION_ENV, Calibration, SubmittedWork, WorkContext, rejected_work,
};

const IDLE: u8 = 0;
const RECORDING: u8 = 1;
const ENCODED: u8 = 2;
const PENDING: u8 = 3;
const MAPPED: u8 = 4;
const FAILED: u8 = 5;

/// View changes discard profitability, while delayed results only choose future work.
#[derive(Default)]
struct Admission {
    view: Option<Entity>,
    frame: u64,
    policy: Option<VisibilityAdmission>,
    first_frame: u64,
}

impl Admission {
    /// Unknown views and `restart` start with fresh occlusion; unprofitable views draw
    /// conservatively.
    fn select(&mut self, view: Option<Entity>, restart: bool) -> bool {
        self.frame += 1;
        if self.view != view || restart {
            self.view = view;
            self.policy = None;
            self.first_frame = self.frame;
        }
        view.is_some()
            && self
                .policy
                .as_mut()
                .is_none_or(|policy| policy.select_work(self.frame))
    }

    /// Measured results from older view selections cannot admit the current view's work.
    fn observe_profitability(&mut self, view: Entity, frame: u64, profitable: bool) {
        if self.view == Some(view) && frame >= self.first_frame {
            self.policy
                .get_or_insert_default()
                .observe_profitability(frame, profitable);
        }
    }

    /// Feedback from a previous selection cannot change the current view's admission.
    fn observe(&mut self, view: Entity, frame: u64, candidates: usize, hidden: usize) {
        if self.view != Some(view) || frame < self.first_frame || candidates == 0 {
            return;
        }
        self.policy
            .get_or_insert_default()
            .observe(frame, candidates, hidden);
    }
}

/// One sparse asynchronous sample; a pending map skips sampling instead of waiting.
struct Sample {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
    view: Entity,
    frame: u64,
    slots: u32,
    work: Option<(WorkContext, Vec<[u64; STREAM_COUNT]>)>,
}

/// The previous prepared cull and the storage whose visibility history it updates.
struct PreparedHistory {
    view: Entity,
    frame: u64,
    decisions: wgpu::Buffer,
    slots: u32,
    needs_followup: bool,
}

#[derive(Resource, Default)]
pub(super) struct GpuCullFeedback {
    admission: Admission,
    sample: Option<Sample>,
    next_sample: u64,
    prepared: Option<PreparedHistory>,
    encoded_frame: AtomicU64,
    calibration: Option<Calibration>,
    context: Option<WorkContext>,
}

impl GpuCullFeedback {
    /// Loads an opt-in measured calibration; absent or invalid files retain the default policy.
    pub(super) fn from_env() -> Self {
        let calibration = env_compat::var_os(CALIBRATION_ENV)
            .and_then(|path| {
                std::fs::read(path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Calibration>(&bytes).ok())
            })
            .filter(Calibration::valid);
        Self {
            calibration,
            ..Default::default()
        }
    }

    /// Uncalibrated frames do not construct work snapshots or inspect adapter metadata.
    pub(super) fn calibrated(&self) -> bool {
        self.calibration.is_some()
    }

    /// Consumes completed samples for future submission choices without waiting. A changed
    /// `context` restarts admission, so no measured verdict outlives its configuration.
    pub(super) fn select(&mut self, view: Option<Entity>, context: Option<WorkContext>) -> bool {
        let restart = self.calibration.is_some() && self.context != context;
        self.context = context;
        if self.admission.view != view || restart {
            self.next_sample = 0;
        }
        let selected = self.admission.select(view, restart);
        if let Some(sample) = &self.sample {
            match sample.state.load(Ordering::Acquire) {
                MAPPED => {
                    if let Ok(data) = sample
                        .buffer
                        .slice(..u64::from(sample.slots) * 4)
                        .get_mapped_range()
                    {
                        let decisions = bytemuck::cast_slice(&data);
                        let (candidates, hidden) = rejection_counts(decisions);
                        bevy::log::debug!(candidates, hidden, "terrain occlusion feedback");
                        match self.measured_profitability(sample, decisions, candidates) {
                            Some(profitable) => self.admission.observe_profitability(
                                sample.view,
                                sample.frame,
                                profitable,
                            ),
                            None => self.admission.observe(
                                sample.view,
                                sample.frame,
                                candidates,
                                hidden,
                            ),
                        }
                    }
                    sample.buffer.unmap();
                    sample.state.store(IDLE, Ordering::Release);
                }
                FAILED => sample.state.store(IDLE, Ordering::Release),
                _ => {}
            }
        }
        selected
            || self.prepared.as_ref().is_some_and(|previous| {
                Some(previous.view) == view
                    && previous.frame + 1 == self.admission.frame
                    && previous.needs_followup
            })
    }

    /// The calibrated verdict for a sample, or `None` when the count policy must decide because
    /// no candidates, weights or matching measurement are available.
    fn measured_profitability(
        &self,
        sample: &Sample,
        decisions: &[u32],
        candidates: usize,
    ) -> Option<bool> {
        let calibration = self.calibration.as_ref().filter(|_| candidates != 0)?;
        let (context, weights) = sample
            .work
            .as_ref()
            .filter(|(context, _)| self.context.as_ref() == Some(context))?;
        let (candidate_indices, hidden_indices) = rejected_work(decisions, weights);
        let profitable = calibration.profitable(context, hidden_indices)?;
        bevy::log::debug!(
            ?candidate_indices,
            ?hidden_indices,
            profitable,
            "terrain weighted occlusion feedback"
        );
        Some(profitable)
    }

    /// Reserves storage only for occasional selected frames, retaining it between samples.
    pub(super) fn prepare(
        &mut self,
        device: &RenderDevice,
        view: Entity,
        slots: u32,
        decisions: &wgpu::Buffer,
        work: Option<SubmittedWork<'_>>,
    ) {
        let frame = self.admission.frame;
        let warm = self.prepared.as_ref().is_some_and(|previous| {
            previous.view == view
                && previous.frame + 1 == frame
                && previous.slots != 0
                && previous.decisions == *decisions
                && self.encoded_frame.load(Ordering::Acquire) == previous.frame
        });
        let due = slots != 0 && frame >= self.next_sample;
        self.prepared = Some(PreparedHistory {
            view,
            frame,
            slots,
            decisions: decisions.clone(),
            needs_followup: slots != 0 && !warm,
        });
        if !due || !warm {
            return;
        }
        if self
            .sample
            .as_ref()
            .is_some_and(|sample| sample.state.load(Ordering::Acquire) != IDLE)
        {
            return;
        }
        let bytes = u64::from(slots) * 4;
        if self
            .sample
            .as_ref()
            .is_none_or(|sample| sample.buffer.size() < bytes)
        {
            let size = bytes
                .next_power_of_two()
                .min(device.limits().max_buffer_size);
            let Some(buffer) =
                crate::gpu_allocation::checked(device, "terrain rejection feedback", size, || {
                    device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
                        label: Some("terrain rejection feedback"),
                        size,
                        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    })
                })
            else {
                return;
            };
            self.sample = Some(Sample {
                buffer,
                state: Arc::new(AtomicU8::new(IDLE)),
                view,
                frame: 0,
                slots: 0,
                work: None,
            });
        }
        let sample = self.sample.as_mut().expect("sample storage was ensured");
        sample.work = self
            .calibration
            .as_ref()
            .and(self.context.clone())
            .zip(work)
            .map(|(context, work)| (context, work.snapshot()));
        sample.view = view;
        sample.frame = self.admission.frame;
        sample.slots = slots;
        sample.state.store(RECORDING, Ordering::Release);
    }

    /// Marks the late pass as encoded and copies decisions for any reserved sample.
    pub(super) fn encode(&self, encoder: &mut wgpu::CommandEncoder, decisions: &wgpu::Buffer) {
        self.encoded_frame
            .store(self.admission.frame, Ordering::Release);
        let Some(sample) = &self.sample else { return };
        if sample.state.load(Ordering::Acquire) == RECORDING {
            encoder.copy_buffer_to_buffer(
                decisions,
                0,
                &sample.buffer,
                0,
                u64::from(sample.slots) * 4,
            );
            sample.state.store(ENCODED, Ordering::Release);
        }
    }

    /// Maps copied decisions, or retries a reserved sample whose late pass did not run.
    fn finish_frame(&mut self) {
        let Some(sample) = &self.sample else {
            return;
        };
        match sample.state.load(Ordering::Acquire) {
            ENCODED => {
                self.next_sample = sample.frame.saturating_add(PROBE_INTERVAL);
                sample.state.store(PENDING, Ordering::Release);
                let state = sample.state.clone();
                sample
                    .buffer
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        state.store(
                            if result.is_ok() { MAPPED } else { FAILED },
                            Ordering::Release,
                        );
                    });
            }
            RECORDING => {
                sample.state.store(IDLE, Ordering::Release);
                if let Some(prepared) = &mut self.prepared {
                    prepared.needs_followup = true;
                }
            }
            _ => {}
        }
    }
}

/// Requests the map after render submission, without an additional poll or submission.
pub(super) fn submit_feedback(mut feedback: ResMut<GpuCullFeedback>) {
    feedback.finish_frame();
}

/// Counts only live, enabled, frustum-visible candidates and their current-depth rejections.
fn rejection_counts(decisions: &[u32]) -> (usize, usize) {
    decisions
        .iter()
        .fold((0, 0), |(candidates, hidden), decision| {
            let candidate = decision & DECISION_CANDIDATE != 0;
            (
                candidates + usize::from(candidate),
                hidden + usize::from(candidate && decision & DECISION_HIDDEN != 0),
            )
        })
}

#[cfg(test)]
#[path = "feedback_tests.rs"]
mod tests;
