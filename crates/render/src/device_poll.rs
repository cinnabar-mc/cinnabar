//! The render world's single device poll, after every submission of the frame.
//!
//! A poll takes the device's fence and lifetime locks that submits and buffer writes also need,
//! so owners never poll themselves: completion and map callbacks fire here, and readers consume
//! what arrived by the previous frame's poll.
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
        renderer::{PendingCommandBuffers, RenderDevice, RenderQueue, render_system},
    },
};
use std::collections::VecDeque;

#[cfg(test)]
#[path = "device_poll_tests.rs"]
mod tests;

/// Frames of GPU work that may still be queued or executing once a frame's poll returns. It is
/// above the two frames a swap chain queues, so presented windows never wait here.
pub(crate) const MAX_FRAMES_IN_FLIGHT: usize = 3;

/// Submission indices of the frames that may still be on the GPU, oldest first.
#[derive(Resource, Default)]
pub(crate) struct FramesInFlight(VecDeque<wgpu::SubmissionIndex>);

impl FramesInFlight {
    /// Records `submission`, the frame's last submission, and polls the device once: without
    /// blocking while at most [`MAX_FRAMES_IN_FLIGHT`] frames are outstanding, otherwise until
    /// the oldest one completes. Callbacks of every finished submission fire before it returns.
    pub(crate) fn finish_frame(
        &mut self,
        device: &wgpu::Device,
        submission: wgpu::SubmissionIndex,
    ) -> Result<wgpu::PollStatus, wgpu::PollError> {
        self.0.push_back(submission);
        let poll = if self.0.len() > MAX_FRAMES_IN_FLIGHT {
            PollType::Wait {
                submission_index: self.0.pop_front(),
                timeout: None,
            }
        } else {
            PollType::Poll
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
#[derive(Resource, Default, Debug)]
pub(crate) struct DevicePolls(pub(crate) u64);

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

fn poll_device(
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    pending: Option<ResMut<PendingCommandBuffers>>,
    mut frames: ResMut<FramesInFlight>,
    mut polls: ResMut<DevicePolls>,
) {
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!("render.device_poll").entered();
    let submission = frame_submission(&queue, pending.map(ResMut::into_inner));
    if let Err(error) = frames.finish_frame(device.wgpu_device(), submission) {
        warn!(?error, "could not poll the render device");
    }
    polls.0 += 1;
}
