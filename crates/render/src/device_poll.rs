//! The render world's single device poll, after every submission of the frame.
//!
//! A poll takes the device's fence and lifetime locks that submits and buffer writes also need,
//! so owners never poll themselves: completion and map callbacks fire here, and readers consume
//! what arrived by the previous frame's poll.
//!
//! The frame's graph submission doubles as its marker, so a frame normally submits once.
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
            PendingCommandBuffers, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue,
            encode_screenshot_and_readback_commands, render_system,
        },
    },
};
use std::collections::VecDeque;

#[cfg(test)]
#[path = "device_poll_tests.rs"]
mod tests;

/// Frames of GPU work that may still be queued or executing once a frame's poll returns. It is
/// above the two frames a swap chain queues, so presented windows never wait here.
pub(crate) const MAX_FRAMES_IN_FLIGHT: usize = 3;

/// Submission markers of the frames that may still be on the GPU, oldest first.
#[derive(Resource, Default)]
pub(crate) struct FramesInFlight {
    queued: VecDeque<wgpu::SubmissionIndex>,
    /// This frame's own graph submission, which marks the frame without another submission.
    frame: Option<wgpu::SubmissionIndex>,
}

impl FramesInFlight {
    /// Records the frame's graph submission as its marker.
    pub(crate) fn record_submission(&mut self, index: wgpu::SubmissionIndex) {
        self.frame = Some(index);
    }

    /// Marks the end of this frame's submissions and polls the device once: without blocking
    /// while at most [`MAX_FRAMES_IN_FLIGHT`] frames are outstanding, otherwise until the oldest
    /// one completes. Callbacks of every finished submission fire before it returns. A frame
    /// without a recorded graph submission is marked by an empty one.
    pub(crate) fn finish_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<wgpu::PollStatus, wgpu::PollError> {
        let marker = self.frame.take().unwrap_or_else(|| queue.submit([]));
        self.queued.push_back(marker);
        let poll = if self.queued.len() > MAX_FRAMES_IN_FLIGHT {
            PollType::Wait {
                submission_index: self.queued.pop_front(),
                timeout: None,
            }
        } else {
            PollType::Poll
        };
        device.poll(poll)
    }
}

/// Owners whose systems register completion callbacks or request readbacks after the graph.
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
        )
        .add_systems(
            RenderGraph,
            submit_frame
                .in_set(RenderGraphSystems::Submit)
                .before(encode_screenshot_and_readback_commands),
        );
}

/// Submits the graph's recorded command buffers and keeps the submission as the frame's marker,
/// sparing the poll an empty submission. Screenshot and readback copies, encoded after it, reach
/// the GPU in Bevy's own later submission.
fn submit_frame(
    mut pending: ResMut<PendingCommandBuffers>,
    queue: Res<RenderQueue>,
    mut frames: ResMut<FramesInFlight>,
) {
    let mut buffers = pending.finish().peekable();
    if buffers.peek().is_some() {
        frames.record_submission(queue.submit(buffers));
    }
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

fn poll_device(
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut frames: ResMut<FramesInFlight>,
    mut polls: ResMut<DevicePolls>,
) {
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!("render.device_poll").entered();
    if let Err(error) = frames.finish_frame(device.wgpu_device(), &queue) {
        warn!(?error, "could not poll the render device");
    }
    polls.0 += 1;
}
