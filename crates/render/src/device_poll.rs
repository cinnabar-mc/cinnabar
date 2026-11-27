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
    ecs::system::SystemParam,
    prelude::{IntoScheduleConfigs, Res, ResMut, Resource, SubApp, SystemSet, warn},
    render::{
        Render, RenderSystems,
        render_resource::PollType,
        renderer::{
            FrameSubmission, FrameTailOnRenderThread, PendingCommandBuffers, RenderDevice,
            RenderFrameTail, RenderQueue, SubmitInFrameTail, render_system,
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
#[path = "device_poll/worker_gate.rs"]
pub(crate) mod worker_gate;
pub(crate) use worker_gate::WorkerSubmitGate;

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

/// Hooks run once each frame's last submission is made, before its poll may wait on the GPU.
#[derive(Resource, Default, Clone)]
pub(crate) struct FrameSubmitted(Arc<Mutex<Vec<SubmittedHook>>>);

type SubmittedHook = Box<dyn Fn() + Send + Sync>;

impl FrameSubmitted {
    /// Runs `hook` after every later frame's submission, where the frame's poll runs.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn subscribe(&self, hook: impl Fn() + Send + Sync + 'static) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Box::new(hook));
    }

    /// Runs every subscribed hook.
    pub(crate) fn notify(&self) {
        for hook in self.0.lock().unwrap_or_else(PoisonError::into_inner).iter() {
            hook();
        }
    }
}

/// Polls issued so far; tests read it to prove there is exactly one per frame.
#[derive(Resource, Default, Debug, Clone)]
pub(crate) struct DevicePolls(pub(crate) Arc<AtomicU64>);

impl DevicePolls {
    #[cfg(test)]
    pub(crate) fn count(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Completion callbacks and buffer maps for [`FrameSubmissions`] systems, which run after the
/// render graph. The graph's command buffers may be submitted later, in the render frame tail, so
/// this work waits for that submission instead of attaching to the previous frame's.
#[derive(SystemParam)]
pub struct FrameCompletion<'w> {
    queue: Res<'w, RenderQueue>,
    submission: Option<Res<'w, FrameSubmission>>,
}

impl FrameCompletion<'_> {
    /// Runs `callback` from a device poll once the GPU finishes this frame's submissions.
    pub fn on_frame_complete(&self, callback: impl FnOnce() + Send + 'static) {
        self.work().on_frame_complete(callback);
    }

    /// Runs `work`, such as a `map_async` request for a buffer this frame writes, once this
    /// frame's command buffers are submitted.
    pub fn after_submit(&self, work: impl FnOnce() + Send + 'static) {
        self.work().after_submit(work);
    }

    /// An owned handle for helpers that register this work outside the system.
    pub fn work(&self) -> FrameWork {
        FrameWork {
            queue: self.queue.clone(),
            submission: self.submission.as_deref().cloned(),
        }
    }
}

/// Owned form of [`FrameCompletion`].
#[derive(Clone)]
pub struct FrameWork {
    queue: RenderQueue,
    submission: Option<FrameSubmission>,
}

impl FrameWork {
    /// Work for a render world that submits each frame before registering it.
    #[cfg(test)]
    pub(crate) fn immediate(queue: RenderQueue) -> Self {
        Self {
            queue,
            submission: None,
        }
    }

    /// Runs `callback` from a device poll once the GPU finishes this frame's submissions.
    pub fn on_frame_complete(&self, callback: impl FnOnce() + Send + 'static) {
        let queue = self.queue.clone();
        self.after_submit(move || {
            queue.on_submitted_work_done(callback);
        });
    }

    /// Runs `work` once this frame's command buffers are submitted.
    pub fn after_submit(&self, work: impl FnOnce() + Send + 'static) {
        match &self.submission {
            Some(submission) => submission.after_submit(work),
            None => work(),
        }
    }
}

/// Keeps the render graph's submission on the render thread before the render app is handed
/// back, for diagnostics that submit their own work after the graph and time it.
#[derive(Resource, Default)]
pub(crate) struct SynchronousFrameSubmission;

/// Makes the render world submit every frame before the render app is handed back.
#[cfg(target_os = "macos")]
pub(crate) fn require_synchronous_submission(render_world: &mut bevy::prelude::World) {
    render_world.init_resource::<SynchronousFrameSubmission>();
    render_world.remove_resource::<SubmitInFrameTail>();
}

/// Idempotent, so every owner that relies on callbacks installs it.
pub(crate) fn install(render_app: &mut SubApp) {
    if render_app.world().contains_resource::<DevicePolls>() {
        return;
    }
    // Every completion callback and map request made after the graph goes through
    // FrameCompletion, so the graph's submission may move into the frame tail.
    if !render_app
        .world()
        .contains_resource::<SynchronousFrameSubmission>()
    {
        render_app.init_resource::<SubmitInFrameTail>();
    }
    render_app
        .init_resource::<DevicePolls>()
        .init_resource::<FramesInFlight>()
        .init_resource::<FrameSubmitted>()
        .init_resource::<WorkerSubmitGate>()
        .configure_sets(
            Render,
            FrameSubmissions
                .in_set(RenderSystems::Render)
                .after(render_system),
        )
        .add_systems(
            Render,
            (
                poll_device
                    .in_set(RenderSystems::Render)
                    .after(FrameSubmissions),
                worker_gate::close_worker_submissions
                    .in_set(RenderSystems::Render)
                    .before(render_system),
                worker_gate::open_worker_submissions.in_set(FrameSubmissions),
            ),
        );
}

/// Runs callback from a device poll after all submissions made so far complete. Call it while
/// preparing the frame, before the render graph, to retire storage this frame no longer uses.
/// [`FrameSubmissions`] systems use [`FrameCompletion`] instead, which also waits for this frame's
/// submission.
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
#[allow(clippy::too_many_arguments)]
fn poll_device(
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    pending: Option<ResMut<PendingCommandBuffers>>,
    frames: Res<FramesInFlight>,
    polls: Res<DevicePolls>,
    tail: Option<Res<RenderFrameTail>>,
    on_render_thread: Option<Res<FrameTailOnRenderThread>>,
    tail_submission: Option<Res<FrameSubmission>>,
    submitted_hooks: Res<FrameSubmitted>,
) {
    let tail = tail.filter(|_| on_render_thread.is_some());
    let mut pending = pending.map(ResMut::into_inner);
    // A graph submitted in the frame tail reports its index there, before this poll runs.
    let submitted = pending
        .as_deref_mut()
        .and_then(PendingCommandBuffers::take_submission);
    let tail_submission = tail_submission
        .filter(|_| tail.is_some() && submitted.is_none())
        .map(|submission| submission.clone());
    let submission = match (submitted, &tail_submission) {
        (Some(index), _) => Some(index),
        (None, Some(_)) => None,
        (None, None) => Some(frame_submission(&queue, pending)),
    };
    let queue = queue.clone();
    let (device, frames, polls) = (device.wgpu_device().clone(), frames.clone(), polls.clone());
    let submitted_hooks = submitted_hooks.clone();
    let poll = move || {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!("render.device_poll").entered();
        let submission = submission.unwrap_or_else(|| {
            tail_submission
                .and_then(|submission| submission.take_index())
                .unwrap_or_else(|| queue.submit([]))
        });
        submitted_hooks.notify();
        if let Err(error) = frames.finish_frame(&device, submission) {
            warn!(?error, "could not poll the render device");
        }
        polls.0.fetch_add(1, Ordering::Relaxed);
    };
    match tail {
        Some(tail) => tail.defer(poll),
        None => poll(),
    }
}
