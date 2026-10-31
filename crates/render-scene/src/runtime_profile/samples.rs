use super::{RuntimeStageSample, initialized_mutex};
use std::{sync::Mutex, time::Duration};

#[derive(Debug)]
pub(super) struct StageSampleAccumulator {
    sample: Mutex<RuntimeStageSample>,
}

impl Default for StageSampleAccumulator {
    /// Prepares the sample and its lock before any timed stage runs.
    fn default() -> Self {
        Self {
            sample: initialized_mutex(RuntimeStageSample::default()),
        }
    }
}

impl StageSampleAccumulator {
    /// Accumulates wall and available CPU durations together under one lock.
    pub(super) fn record(&self, elapsed: Duration, thread_cpu: Option<Duration>) {
        let mut sample = self
            .sample
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sample.count = sample.count.saturating_add(1);
        sample.total = sample.total.saturating_add(elapsed);
        sample.maximum = sample.maximum.max(elapsed);
        if let Some(cpu) = thread_cpu {
            sample.thread_cpu_count = sample.thread_cpu_count.saturating_add(1);
            sample.thread_cpu_total = sample.thread_cpu_total.saturating_add(cpu);
            sample.thread_cpu_maximum = sample.thread_cpu_maximum.max(cpu);
        }
    }

    /// Drains one reporting interval without losing paired timing fields.
    pub(super) fn take(&self) -> RuntimeStageSample {
        let mut sample = self
            .sample
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut *sample)
    }
}

impl StageSampleAccumulator {
    /// Copies the current interval without consuming another diagnostic reader's samples.
    pub(super) fn snapshot(&self) -> RuntimeStageSample {
        *self
            .sample
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
