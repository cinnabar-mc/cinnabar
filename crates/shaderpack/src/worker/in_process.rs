//! Cooperative compiler fallback for hosts that cannot start a process.

use super::{OUTPUT_BYTES, Request, Response, TIME_LIMIT, allocator, translate};
use std::{
    io::{self, Write},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

const ACTIVE: u8 = 0;
const CANCELLED: u8 = 1;
const EXPIRED: u8 = 2;
const HEAP: u8 = 3;

/// Shared interruption state; the first failure remains visible until retirement.
struct Control {
    reason: AtomicU8,
    started: Instant,
    heap_epoch: usize,
}

impl Control {
    /// Starts a deadline and observes the installed allocator's live process heap.
    fn new() -> Self {
        Self {
            reason: AtomicU8::new(ACTIVE),
            started: Instant::now(),
            heap_epoch: allocator::heap_epoch(),
        }
    }

    /// Records interruption without replacing an earlier cancellation or resource failure.
    fn stop(&self, reason: u8) {
        let _ = self
            .reason
            .compare_exchange(ACTIVE, reason, Ordering::AcqRel, Ordering::Acquire);
    }

    /// Checks the deadline and soft heap budget at a cooperative translation boundary.
    fn cancelled(&self) -> bool {
        if self.started.elapsed() >= TIME_LIMIT {
            self.stop(EXPIRED);
        }
        if allocator::heap_exceeded(self.heap_epoch) {
            self.stop(HEAP);
        }
        self.reason.load(Ordering::Acquire) != ACTIVE
    }

    /// Gives the host a stable reason, even when a later poll no longer requests cancellation.
    fn error(&self) -> Option<String> {
        match self.reason.load(Ordering::Acquire) {
            CANCELLED => Some("shader pack load cancelled".into()),
            EXPIRED => Some("shader compiler exceeded its time limit".into()),
            HEAP => Some("shader compiler exceeded its heap budget".into()),
            _ => None,
        }
    }
}

/// One joined compiler thread; unfinished work is retained during normal polling.
pub(super) struct Running {
    control: Arc<Control>,
    worker: Option<JoinHandle<Result<Response, String>>>,
}

impl Running {
    /// Starts the same translation as stdio mode without opening any child process.
    pub(super) fn start(input: Vec<u8>) -> Result<Self, String> {
        Self::spawn(move |control| {
            let request: Request =
                serde_json::from_slice(&input).map_err(|error| error.to_string())?;
            let result = translate(request, &|| control.cancelled());
            // Validate the identical wire-size cap without retaining a second copy of the result.
            ciborium::into_writer(
                &result,
                super::LimitedWriter {
                    writer: CheckedWriter(control),
                    remaining: OUTPUT_BYTES,
                },
            )
            .map_err(|error| error.to_string())?;
            result
        })
    }

    /// Owns background work and checks resource failures again before publishing its result.
    fn spawn(
        work: impl FnOnce(&Control) -> Result<Response, String> + Send + 'static,
    ) -> Result<Self, String> {
        let control = Arc::new(Control::new());
        let thread_control = Arc::clone(&control);
        let worker = std::thread::Builder::new()
            .name("shader compiler".into())
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                // Pack translation uses Rayon. Its queue must not occupy rendering's shared pool.
                let result = rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .thread_name(|_| "shader translation".into())
                    .stack_size(64 * 1024 * 1024)
                    .build()
                    .map_err(|error| format!("cannot start shader translation pool: {error}"))
                    .and_then(|pool| pool.install(|| work(&thread_control)));
                thread_control.cancelled();
                thread_control.error().map_or(result, Err)
            })
            .map_err(|error| format!("cannot start shader compiler thread: {error}"))?;
        Ok(Self {
            control,
            worker: Some(worker),
        })
    }

    /// Requests interruption and returns only after translation has relinquished its thread.
    pub(super) fn poll(
        &mut self,
        elapsed: Duration,
        cancelled: bool,
    ) -> Option<Result<Response, String>> {
        if cancelled {
            self.control.stop(CANCELLED);
        } else if elapsed >= TIME_LIMIT {
            self.control.stop(EXPIRED);
        }
        if !self.worker.as_ref()?.is_finished() {
            return None;
        }
        let result = self
            .worker
            .take()
            .unwrap()
            .join()
            .unwrap_or_else(|_| Err("shader compiler thread stopped".into()));
        Some(self.control.error().map_or(result, Err))
    }
}

impl Drop for Running {
    /// Cancels and joins only owned work; polling hosts retire it before final teardown.
    fn drop(&mut self) {
        self.control.stop(CANCELLED);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Counts serialized bytes without storing them and checks interruption during serialization.
struct CheckedWriter<'a>(&'a Control);

impl Write for CheckedWriter<'_> {
    /// Stops serialization promptly when the compiler has been interrupted.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.cancelled() {
            return Err(io::Error::other("shader compilation interrupted"));
        }
        Ok(bytes.len())
    }

    /// No buffered output needs flushing.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
