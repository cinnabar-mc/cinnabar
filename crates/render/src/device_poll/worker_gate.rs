//! Submissions from background GPU workers, held back while the frame's own submission is
//! deferred into the render frame tail.
//!
//! [`SubmitInFrameTail`](bevy::render::renderer::SubmitInFrameTail) forbids any other submission
//! between the render graph and the frame tail, since it would reach the queue ahead of the
//! frame's command buffers. A worker that builds GPU resources off the render thread, such as a
//! resource-pack reload's candidate arena, submits through [`submit`] inside [`with_gate`], which
//! waits while the render thread holds the gate closed.

use bevy::{
    prelude::{Res, Resource},
    render::renderer::RenderQueue,
};
use std::{
    cell::RefCell,
    sync::{Arc, Condvar, Mutex, PoisonError},
};

/// Closed from just before the render graph runs until the frame's own submission is made.
#[derive(Resource, Clone, Default)]
pub(crate) struct WorkerSubmitGate(Arc<Gate>);

#[derive(Default)]
struct Gate {
    closed: Mutex<bool>,
    opened: Condvar,
    /// Workers waiting for the gate to open, for tests that prove a submission was held back.
    #[cfg(test)]
    waiting: std::sync::atomic::AtomicUsize,
}

impl WorkerSubmitGate {
    /// Holds back worker submissions; waits for one already being made.
    fn close(&self) {
        *self.0.closed.lock().unwrap_or_else(PoisonError::into_inner) = true;
    }

    /// Lets held-back worker submissions reach the queue.
    fn open(&self) {
        *self.0.closed.lock().unwrap_or_else(PoisonError::into_inner) = false;
        self.0.opened.notify_all();
    }

    /// Submits `commands` once the gate is open, keeping it open until the submission is made.
    fn submit(
        &self,
        queue: &RenderQueue,
        commands: impl IntoIterator<Item = wgpu::CommandBuffer>,
    ) -> wgpu::SubmissionIndex {
        let mut closed = self.0.closed.lock().unwrap_or_else(PoisonError::into_inner);
        while *closed {
            #[cfg(test)]
            self.0
                .waiting
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            closed = self
                .0
                .opened
                .wait(closed)
                .unwrap_or_else(PoisonError::into_inner);
        }
        queue.submit(commands)
    }
}

thread_local! {
    static WORKER_GATE: RefCell<Option<WorkerSubmitGate>> = const { RefCell::new(None) };
}

/// Runs `work` with this thread's [`submit`] calls waiting on `gate`.
pub(crate) fn with_gate<T>(gate: WorkerSubmitGate, work: impl FnOnce() -> T) -> T {
    struct Restore(Option<WorkerSubmitGate>);
    impl Drop for Restore {
        /// Restores the previous gate even when `work` panics.
        fn drop(&mut self) {
            let previous = self.0.take();
            WORKER_GATE.with_borrow_mut(|gate| *gate = previous);
        }
    }
    let _restore = Restore(WORKER_GATE.with_borrow_mut(|slot| slot.replace(gate)));
    work()
}

/// Submits `commands`, waiting for the frame's own submission first when this thread is a
/// background worker inside [`with_gate`]. The render thread submits at once.
pub(crate) fn submit(
    queue: &RenderQueue,
    commands: impl IntoIterator<Item = wgpu::CommandBuffer>,
) -> wgpu::SubmissionIndex {
    match WORKER_GATE.with_borrow(Clone::clone) {
        Some(gate) => gate.submit(queue, commands),
        None => queue.submit(commands),
    }
}

/// Closes the gate before the render graph, whose submission may be deferred into the tail.
pub(super) fn close_worker_submissions(gate: Res<WorkerSubmitGate>) {
    gate.close();
}

/// Reopens the gate once the frame's submission is made: in the frame tail when it was deferred,
/// otherwise at once.
pub(super) fn open_worker_submissions(
    gate: Res<WorkerSubmitGate>,
    completion: super::FrameCompletion,
) {
    let gate = gate.clone();
    completion.after_submit(move || gate.open());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn queue() -> RenderQueue {
        let (_, queue) = wgpu::Device::noop(&Default::default());
        RenderQueue::new(queue)
    }

    /// A worker's submission waits while the gate is closed and reaches the queue once it opens.
    #[test]
    fn a_worker_submission_waits_for_the_open_gate() {
        let gate = WorkerSubmitGate::default();
        gate.close();
        let queue = queue();
        let (done, submitted) = std::sync::mpsc::channel();
        let worker_gate = gate.clone();
        let worker = std::thread::spawn(move || {
            with_gate(worker_gate, || submit(&queue, []));
            done.send(()).unwrap();
        });
        test_time::eventually("the worker to wait on the closed gate", || {
            gate.0.waiting.load(Ordering::Relaxed) > 0
        });
        assert!(
            submitted.try_recv().is_err(),
            "nothing is submitted while the gate is closed"
        );
        gate.open();
        test_time::eventually("the held-back submission", || submitted.try_recv().is_ok());
        worker.join().unwrap();
    }

    /// The render thread is never a gated worker, so its own submissions go through at once.
    #[test]
    fn submissions_outside_a_worker_ignore_the_gate() {
        let gate = WorkerSubmitGate::default();
        gate.close();
        submit(&queue(), []);
        assert_eq!(gate.0.waiting.load(Ordering::Relaxed), 0);
    }
}
