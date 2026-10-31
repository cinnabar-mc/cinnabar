//! Native calling-thread CPU counters, separate from elapsed wall time.

use std::time::Duration;

/// Reads cumulative user and kernel CPU time for the calling thread.
/// Unsupported platforms and failed reads return `None`. Values use nanoseconds;
/// Windows reports 100 ns units, whose effective update resolution can be coarser.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
pub fn thread_cpu_time() -> Option<Duration> {
    use rustix::time::{ClockId, DynamicClockId, clock_gettime_dynamic};
    let time = clock_gettime_dynamic(DynamicClockId::Known(ClockId::ThreadCPUTime)).ok()?;
    Some(Duration::new(
        u64::try_from(time.tv_sec).ok()?,
        u32::try_from(time.tv_nsec).ok()?,
    ))
}

/// Reads cumulative user and kernel CPU time for the calling thread.
/// Windows reports 100 ns units, whose effective update resolution can be coarser.
#[cfg(windows)]
pub fn thread_cpu_time() -> Option<Duration> {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentThread, GetThreadTimes},
    };
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: The pseudo-handle refers to this thread and each output is writable.
    if unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return None;
    }
    Some(filetime_duration(kernel).saturating_add(filetime_duration(user)))
}

/// Converts a Windows duration counter from 100 ns units without overflowing.
#[cfg(windows)]
fn filetime_duration(time: windows_sys::Win32::Foundation::FILETIME) -> Duration {
    let ticks = (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    const TICKS_PER_SECOND: u64 = 10_000_000;
    Duration::new(
        ticks / TICKS_PER_SECOND,
        ((ticks % TICKS_PER_SECOND) * 100) as u32,
    )
}

/// Leaves CPU timing unavailable on platforms without a supported thread clock.
#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "android",
    windows
)))]
pub fn thread_cpu_time() -> Option<Duration> {
    None
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Both halves of FILETIME contribute without overflowing nanoseconds.
    #[test]
    fn windows_cpu_units_preserve_large_counters() {
        assert_eq!(
            filetime_duration(windows_sys::Win32::Foundation::FILETIME {
                dwHighDateTime: 1,
                dwLowDateTime: 1,
            }),
            Duration::from_nanos(((1_u64 << 32) + 1) * 100)
        );
        assert_eq!(
            filetime_duration(windows_sys::Win32::Foundation::FILETIME {
                dwHighDateTime: u32::MAX,
                dwLowDateTime: u32::MAX,
            }),
            Duration::new(
                u64::MAX / 10_000_000,
                ((u64::MAX % 10_000_000) * 100) as u32
            )
        );
    }
}
