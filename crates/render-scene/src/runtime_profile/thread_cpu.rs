//! Optional thread CPU sampling and paired completed-span totals for spike output.

use super::{RuntimeStage, initialized_mutex};
use std::{fmt::Write, sync::Mutex, thread::ThreadId, time::Duration};

#[derive(Debug, Clone, Copy)]
pub(super) struct ThreadCpuStart {
    thread: ThreadId,
    time: Duration,
}

#[derive(Debug)]
pub(super) struct ThreadCpuProfiler {
    clock: fn() -> Option<Duration>,
    frame: Mutex<ThreadCpuFrame>,
}

impl ThreadCpuProfiler {
    /// Prepares fixed-size counters before any timed span runs.
    pub(super) fn new(clock: fn() -> Option<Duration>) -> Self {
        Self {
            clock,
            frame: initialized_mutex(ThreadCpuFrame::default()),
        }
    }

    /// Captures a native counter with the thread that owns its epoch.
    pub(super) fn start(&self) -> Option<ThreadCpuStart> {
        Some(ThreadCpuStart {
            thread: std::thread::current().id(),
            time: (self.clock)()?,
        })
    }

    /// Rejects moved spans, failed reads and counters that moved backwards.
    pub(super) fn elapsed(&self, start: ThreadCpuStart) -> Option<Duration> {
        if start.thread != std::thread::current().id() {
            return None;
        }
        (self.clock)()?.checked_sub(start.time)
    }

    /// Accumulates only spans for which both wall and CPU time were measured.
    pub(super) fn record(&self, stage: RuntimeStage, wall: Duration, cpu: Duration) {
        let mut frame = self
            .frame
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sample = &mut frame.0[stage as usize];
        sample.count = sample.count.saturating_add(1);
        sample.wall = sample.wall.saturating_add(wall);
        sample.cpu = sample.cpu.saturating_add(cpu);
    }

    /// Drains completed samples into an owned report without formatting while locked.
    pub(super) fn finish_frame(&self) -> ThreadCpuFrame {
        std::mem::take(
            &mut *self
                .frame
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

/// Subtracts the CPU part of an excluded wait only when its thread is comparable.
pub(super) fn excluding_wait(
    total: Duration,
    wall_wait: Duration,
    cpu_wait: Option<(ThreadId, Duration)>,
) -> Option<Duration> {
    if wall_wait.is_zero() {
        return Some(total);
    }
    let (thread, wait) = cpu_wait?;
    if thread != std::thread::current().id() {
        return None;
    }
    total.checked_sub(wait)
}

#[derive(Debug, Clone, Copy, Default)]
struct Sample {
    count: u64,
    wall: Duration,
    cpu: Duration,
}

#[derive(Debug)]
pub(super) struct ThreadCpuFrame([Sample; RuntimeStage::ALL.len()]);

impl Default for ThreadCpuFrame {
    /// Initializes one slot per stage without allocating.
    fn default() -> Self {
        Self([Sample::default(); RuntimeStage::ALL.len()])
    }
}

impl ThreadCpuFrame {
    /// Appends stage, measured count, wall ms and CPU ms; overlapping stages are inclusive.
    pub(super) fn append_fields(&self, fields: &mut String) {
        let mut separator = " stage_thread_cpu=";
        for (stage, sample) in RuntimeStage::ALL.into_iter().zip(self.0) {
            if sample.count == 0 {
                continue;
            }
            let _ = write!(
                fields,
                "{separator}{}:{}:{:.3}:{:.3}",
                stage.name(),
                sample.count,
                sample.wall.as_secs_f64() * 1e3,
                sample.cpu.as_secs_f64() * 1e3
            );
            separator = ",";
        }
    }
}
