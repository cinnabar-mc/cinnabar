//! The process's CPU plan, shared by every thread pool: how many cores pools size themselves
//! for, and which cores background threads may run on.
//!
//! Phones pair one to four fast cores with slower ones. On Android the plan reserves the
//! fastest cores for the main and render threads, keeps background threads off them, and
//! sizes pools by what the remaining cores can do instead of by the core count. Uniform or
//! unreadable topologies, and every desktop by default, keep sizing by the logical-processor
//! count with no affinity.

#[cfg(any(target_os = "linux", target_os = "android"))]
mod affinity;
mod overrides;
mod plan;
#[cfg(test)]
mod tests;
mod topology;

pub use plan::{CpuPlan, TopologyMode};
pub use topology::{BackgroundPlacement, CpuTopology, SpeedSource};

/// Cores left to the frame: the main and render threads.
pub const FRAME_CORES: usize = 2;

/// The process's plan, detected once on first use from sysfs, the overrides and, off Android,
/// the first caller's affinity.
pub fn plan() -> &'static CpuPlan {
    static PLAN: std::sync::LazyLock<CpuPlan> = std::sync::LazyLock::new(CpuPlan::detect);
    &PLAN
}

/// Confines the calling thread to the plan's background CPUs, leaving the frame cores to the
/// main and render threads, and remembers it for [`refresh_background_threads`]. Threads the
/// caller spawns later inherit the confinement. Does nothing when the plan reserves no frame
/// cores. A failure keeps the thread's placement and warns once per process.
///
/// Only threads no frame thread waits on belong here: a confined thread runs several times
/// slower than one on a frame core.
pub fn enter_background_cores() {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if let Some(placement) = plan().placement()
        && let Err(error) = affinity::BACKGROUND.confine_current_thread(&placement.background)
    {
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| {
            tracing::warn!(%error, "could not keep background threads off the frame cores");
        });
    }
}

/// Confines again every live thread that called [`enter_background_cores`].
///
/// Android moves an app between cpusets as it enters and leaves the foreground, and kernels
/// before 6.2 reset each thread's affinity to the whole cpuset when they do, so the caller
/// repeats this from time to time. Failures are ignored: a cpuset that excludes every
/// background CPU already keeps the threads off the frame cores.
pub fn refresh_background_threads() {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if let Some(placement) = plan().placement() {
        affinity::BACKGROUND.confine_registered_threads(&placement.background);
    }
}
