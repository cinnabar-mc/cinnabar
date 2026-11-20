//! Native world threads, priority and monotonic sampling.

use super::*;
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Samples elapsed native time from one fixed process-local origin.
pub(super) fn now() -> Moment {
    static ORIGIN: LazyLock<std::time::Instant> = LazyLock::new(std::time::Instant::now);
    Moment::from_elapsed(ORIGIN.elapsed())
}

/// Starts the foreground/background native pools without changing their widths or priority.
pub(super) fn start(shared: &Arc<Shared>, size: PoolSize) {
    for (name, count, background) in [
        ("world", size.foreground, false),
        ("world-bg", size.background, true),
    ] {
        for index in 0..count {
            let shared = Arc::clone(shared);
            std::thread::Builder::new()
                .name(format!("{name}-{index}"))
                .spawn(move || work(&shared, name, background))
                .expect("world worker could not start");
        }
    }
}

/// Drains finite jobs on a native worker, retaining its scratch and priority policy. Every
/// worker stays off the frame cores the process's CPU plan reserves; nothing on the frame
/// threads waits for a world job.
fn work(shared: &Shared, name: &str, background: bool) {
    cpu_topology::enter_background_cores();
    let per_job = background
        && priority::begin_background().unwrap_or_else(|error| {
            tracing::warn!("{name}: could not lower worker priority: {error}");
            false
        });
    let mut scratch = WorkerScratch::default();
    let mut queues = shared.lock();
    loop {
        if queues.shutdown {
            return;
        }
        let observed = now();
        let Some(Taken { job, below_frame }) = queues.take(background, observed) else {
            queues = shared.wait(queues, scratch.idle_wait(observed));
            drop(queues);
            scratch.release_if_idle(now());
            queues = shared.lock();
            continue;
        };
        drop(queues);
        LOWERED_FOR_JOB.set(below_frame && per_job && priority::lower_for_job().is_ok());
        // Matches rayon's default: a panicking world job aborts rather than losing its permits.
        if catch_unwind(AssertUnwindSafe(|| job(&mut scratch))).is_err() {
            std::process::abort();
        }
        restore_lowered_job();
        scratch.finished(now());
        queues = shared.lock();
    }
}

thread_local! {
    /// Whether this worker runs its current job lowered and must restore normal priority.
    static LOWERED_FOR_JOB: Cell<bool> = const { Cell::new(false) };
}

/// Returns a worker lowered for its job to normal priority; does nothing otherwise.
pub(super) fn restore_lowered_job() {
    if LOWERED_FOR_JOB.replace(false)
        && let Err(error) = priority::restore()
    {
        let name = std::thread::current().name().unwrap_or("world").to_owned();
        tracing::warn!("{name}: could not restore worker priority: {error}");
    }
}
