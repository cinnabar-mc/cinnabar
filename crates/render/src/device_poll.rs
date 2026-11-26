//! The render world's single device poll, after every submission of the frame.
//!
//! A poll takes the device's fence and lifetime locks that submits and buffer writes also need,
//! so owners never poll themselves: completion and map callbacks fire here, and readers consume
//! what arrived by the previous frame's poll. With a render thread, the poll runs in the render
//! frame tail, after the render app is handed back and before the next render frame.
//!
//! The poll also bounds how far the CPU runs ahead of the GPU. Every queued frame keeps its
//! command lists, queue-write staging and indirect-validation buffers until the GPU finishes it.
//! A presented window waits on its swap chain, but hidden clients render with none, so without
//! this bound a join or another GPU-heavy process lets frames queue until the driver runs out of
//! memory.

use bevy::{
    prelude::{IntoScheduleConfigs, Res, ResMut, Resource, SubApp, SystemSet, warn},
    render::{
        Render, RenderSystems,
        render_resource::PollType,
        renderer::{
            FrameTailOnRenderThread, PendingCommandBuffers, RenderDevice, RenderFrameTail,
            RenderQueue, render_system,
        },
    },
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

#[cfg(test)]
#[path = "device_poll_tests.rs"]
mod tests;

/// Frames of GPU work that may still be queued or executing once a frame's poll returns. It is
/// above the two frames a swap chain queues, so presented windows never wait here.
pub(crate) const MAX_FRAMES_IN_FLIGHT: usize = 3;

/// Submission indices of the frames that may still be on the GPU, oldest first. Shared, so the poll
/// can run in the render frame tail after the render app is handed back.
#[derive(Resource, Default, Clone)]
pub(crate) struct FramesInFlight(pub(crate) Arc<Mutex<VecDeque<wgpu::SubmissionIndex>>>);

impl FramesInFlight {
    /// Records `submission`, the frame's last submission, and polls the device once: without
    /// blocking while at most [`MAX_FRAMES_IN_FLIGHT`] frames are outstanding, otherwise until
    /// the oldest one completes. Callbacks of every finished submission fire before it returns.
    pub(crate) fn finish_frame(
        &self,
        device: &wgpu::Device,
        submission: wgpu::SubmissionIndex,
    ) -> Result<wgpu::PollStatus, wgpu::PollError> {
        let poll = {
            let mut frames = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            frames.push_back(submission);
            if frames.len() > MAX_FRAMES_IN_FLIGHT {
                PollType::Wait {
                    submission_index: frames.pop_front(),
                    timeout: None,
                }
            } else {
                PollType::Poll
            }
        };
        device.poll(poll)
    }
}

/// Owners whose systems register completion callbacks or request readbacks after the graph.
///
/// Work these systems submit must go through [`PendingCommandBuffers::submit`], so the frame's
/// bound covers it.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrameSubmissions;

/// Polls issued so far; tests read it to prove there is exactly one per frame.
#[derive(Resource, Default, Debug, Clone)]
pub(crate) struct DevicePolls(pub(crate) Arc<AtomicU64>);

impl DevicePolls {
    #[cfg(test)]
    pub(crate) fn count(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Idempotent, so every owner that relies on callbacks installs it.
pub(crate) fn install(render_app: &mut SubApp) {
    if render_app.world().contains_resource::<DevicePolls>() {
        return;
    }
    render_app
        .init_resource::<DevicePolls>()
        .init_resource::<FramesInFlight>()
        .configure_sets(
            Render,
            FrameSubmissions
                .in_set(RenderSystems::Render)
                .after(render_system),
        )
        .add_systems(
            Render,
            poll_device
                .in_set(RenderSystems::Render)
                .after(FrameSubmissions),
        );
}

/// Runs callback from a device poll after all submissions so far, including this frame, complete.
/// Call after the resource's last submission. Before rendering, only retire storage that
/// this frame no longer uses; FrameSubmissions systems may also retire this frame's storage.
pub(crate) fn on_frame_complete(queue: &RenderQueue, callback: impl FnOnce() + Send + 'static) {
    queue.on_submitted_work_done(callback);
}

/// Reads a reserved queue index with an empty submit; callback registration may reserve one too.
#[cfg(test)]
pub(crate) fn submission_marker(queue: &RenderQueue) -> u64 {
    let index = format!("{:?}", queue.submit([]));
    index
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("submission index")
}

/// The index that bounds this frame: its last submission of pending command buffers, which
/// carries every write staged before it. A frame without one submits an empty batch instead,
/// which sends the writes staged since the previous submission. An empty submission is a full
/// driver submission on RADV, so frames that submitted skip it; writes staged after their
/// submission go out with the next frame's.
fn frame_submission(
    queue: &RenderQueue,
    pending: Option<&mut PendingCommandBuffers>,
) -> wgpu::SubmissionIndex {
    pending
        .and_then(PendingCommandBuffers::take_submission)
        .unwrap_or_else(|| queue.submit([]))
}

/// Takes the frame's submission index here, where every submission of the frame is known, and
/// polls in the render frame tail when the render thread runs one: the poll and the callbacks it
/// fires need no render world, so the next extraction overlaps them. They still run before the
/// next render frame, so readers keep seeing what arrived by the previous frame's poll.
fn poll_device(
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    pending: Option<ResMut<PendingCommandBuffers>>,
    frames: Res<FramesInFlight>,
    polls: Res<DevicePolls>,
    tail: Option<Res<RenderFrameTail>>,
    on_render_thread: Option<Res<FrameTailOnRenderThread>>,
) {
    let submission = frame_submission(&queue, pending.map(ResMut::into_inner));
    let (device, frames, polls) = (device.wgpu_device().clone(), frames.clone(), polls.clone());
    let poll = move || {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!("render.device_poll").entered();
        if let Err(error) = frames.finish_frame(&device, submission) {
            warn!(?error, "could not poll the render device");
        }
        polls.0.fetch_add(1, Ordering::Relaxed);
    };
    match tail.filter(|_| on_render_thread.is_some()) {
        Some(tail) => tail.defer(poll),
        None => poll(),
    }
}
