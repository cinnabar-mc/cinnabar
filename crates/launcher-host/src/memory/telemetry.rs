//! Logs one memory line per reported event, from a thread of its own: walking the kernel's
//! accounting for this process and its helpers takes milliseconds, which no frame should pay.
//! The thread also logs a `periodic` line whenever [`PERIOD`] passes without one, so lines keep
//! coming while frames stall, such as during shader compilation or after the app is hidden.

use std::{
    sync::{
        Mutex, OnceLock, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crossbeam_channel::{RecvTimeoutError, Sender, TrySendError};

use super::{
    proc,
    snapshot::{ChildMemory, Counters, Snapshot},
};
use crate::lifecycle::children;

/// Events waiting for the reporter; later ones are dropped rather than waited for.
const QUEUED_EVENTS: usize = 32;
/// The longest the reporter goes without a line.
pub const PERIOD: Duration = Duration::from_secs(30);
/// The event name of a line logged because [`PERIOD`] passed without one.
pub const PERIODIC_EVENT: &str = "periodic";

static EVENTS: OnceLock<Sender<String>> = OnceLock::new();
/// The newest totals from the client's subsystems, reported with every event.
static COUNTERS: Mutex<Counters> = Mutex::new(Counters::new());
/// Set once the platform's own memory figures can be read.
static PLATFORM_READY: AtomicBool = AtomicBool::new(false);
/// Lists the process ids of helper processes started outside [`children`].
type ChildSource = fn() -> Vec<u32>;

/// Sources of helper processes started outside [`children`], such as shader compilers.
static CHILD_SOURCES: Mutex<Vec<ChildSource>> = Mutex::new(Vec::new());

/// Starts the reporter thread; later calls do nothing. Until it is started, [`report`] is a
/// no-op, so tools and tests that never start it log nothing.
pub fn start() {
    EVENTS.get_or_init(|| {
        let (events, received) = crossbeam_channel::bounded::<String>(QUEUED_EVENTS);
        let spawned = std::thread::Builder::new()
            .name("memory-telemetry".to_owned())
            .spawn(move || {
                let mut last_line = Instant::now();
                loop {
                    let event = match received.recv_timeout(next_wait(last_line.elapsed())) {
                        Ok(event) => event,
                        Err(RecvTimeoutError::Timeout) => PERIODIC_EVENT.to_owned(),
                        Err(RecvTimeoutError::Disconnected) => break,
                    };
                    tracing::info!("{}", read().line(&event));
                    last_line = Instant::now();
                }
            });
        if let Err(error) = spawned {
            tracing::warn!("memory lines are unavailable: {error}");
        }
        events
    });
}

/// Asks for one memory line labelled `event`, read and logged off the calling thread. Never
/// blocks: an event that finds the queue full is dropped.
pub fn report(event: &str) {
    if let Some(events) = EVENTS.get()
        && let Err(TrySendError::Full(event)) = events.try_send(event.to_owned())
    {
        tracing::debug!("memory line for {event} dropped: the reporter is behind");
    }
}

/// Lets later lines include the platform's own memory figures. Android publishes the context
/// they are read through only once the game activity runs, after the setup activity has already
/// started this reporter, so the client calls this when it builds its app.
pub fn platform_ready() {
    PLATFORM_READY.store(true, Ordering::Release);
}

/// Adds `source`, which lists process ids of helper processes the client started without
/// [`children`], to every later line's `children` group. A process listed more than once is
/// reported once, and ids of exited processes are skipped.
pub fn add_child_source(source: ChildSource) {
    CHILD_SOURCES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(source);
}

/// How long the reporter waits for an event when its last line was `since_last_line` ago.
fn next_wait(since_last_line: Duration) -> Duration {
    PERIOD.saturating_sub(since_last_line)
}

/// Replaces the subsystem totals that later lines report.
pub fn set_counters(counters: Counters) {
    *COUNTERS.lock().unwrap_or_else(PoisonError::into_inner) = counters;
}

/// Reads this process, its tracked helpers, the device and the platform now.
fn read() -> Snapshot {
    let sources = CHILD_SOURCES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let mut pids = children::live_pids();
    for source in sources {
        pids.extend(source());
    }
    pids.sort_unstable();
    pids.dedup();
    let children = pids
        .into_iter()
        .filter_map(|pid| {
            let memory = proc::process(Some(pid));
            (!memory.is_empty()).then(|| ChildMemory {
                name: proc::name(pid).unwrap_or_default(),
                pid,
                memory,
            })
        })
        .collect();
    Snapshot {
        process: proc::process(None),
        children,
        system: proc::system(),
        platform: platform(),
        counters: COUNTERS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone(),
    }
}

/// Android's own availability figures, once [`platform_ready`] allows reading them.
#[cfg(target_os = "android")]
fn platform() -> Option<super::snapshot::PlatformMemory> {
    PLATFORM_READY
        .load(Ordering::Acquire)
        .then(super::android::memory_info)
        .flatten()
}

/// Only Android has availability figures of its own that the client reads.
#[cfg(not(target_os = "android"))]
fn platform() -> Option<super::snapshot::PlatformMemory> {
    None
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn the_reporter_waits_out_the_rest_of_the_period() {
        assert_eq!(next_wait(Duration::ZERO), PERIOD);
        assert_eq!(
            next_wait(PERIOD - Duration::from_secs(1)),
            Duration::from_secs(1)
        );
        assert_eq!(next_wait(PERIOD), Duration::ZERO);
        assert_eq!(next_wait(PERIOD * 2), Duration::ZERO);
    }

    /// Lists this test process as a helper.
    fn this_process() -> Vec<u32> {
        vec![std::process::id(), u32::MAX]
    }

    #[test]
    fn added_helper_processes_are_read_and_exited_ones_skipped() {
        add_child_source(this_process);
        add_child_source(this_process);
        let children = read().children;
        let own: Vec<_> = children
            .iter()
            .filter(|child| child.pid == std::process::id())
            .collect();
        assert_eq!(own.len(), 1, "{children:?}");
        assert!(own[0].memory.resident.is_some(), "{children:?}");
        assert!(children.iter().all(|child| child.pid != u32::MAX));
    }

    #[test]
    fn a_reading_covers_this_process_and_the_device() {
        let snapshot = read();
        assert!(snapshot.process.resident.is_some(), "{snapshot:?}");
        assert!(snapshot.system.total.is_some(), "{snapshot:?}");
        assert_eq!(snapshot.platform, None);
        let line = snapshot.line("test");
        assert!(line.starts_with("memory event=test rss="), "{line}");
        assert!(line.contains(" system=[available="), "{line}");
    }
}
