//! Reads the kernel's memory accounting for this process and its helpers. Linux and Android
//! expose it under `/proc`; other systems report nothing.

use super::snapshot::{ProcessMemory, SystemMemory};

/// Memory of process `pid`, or of this process for `None`. A process of the same user is
/// readable; one that has exited, or a file the system hides, leaves its figures unknown.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn process(pid: Option<u32>) -> ProcessMemory {
    let dir = pid.map_or_else(|| "/proc/self".to_owned(), |pid| format!("/proc/{pid}"));
    let rollup = std::fs::read_to_string(format!("{dir}/smaps_rollup")).unwrap_or_default();
    let memory = ProcessMemory::from_smaps_rollup(&rollup);
    if memory.resident.is_some() {
        return memory;
    }
    let statm = std::fs::read_to_string(format!("{dir}/statm")).unwrap_or_default();
    memory.or_statm(&statm, rustix::param::page_size() as u64)
}

/// The kernel's short name for process `pid`, when it still runs.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn name(pid: u32) -> Option<String> {
    let name = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    Some(name.trim().to_owned())
}

/// The device totals in `/proc/meminfo`.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn system() -> SystemMemory {
    std::fs::read_to_string("/proc/meminfo")
        .map(|meminfo| SystemMemory::from_meminfo(&meminfo))
        .unwrap_or_default()
}

/// No per-process accounting is read on this system.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub(super) fn process(_pid: Option<u32>) -> ProcessMemory {
    ProcessMemory::default()
}

/// No process names are read on this system.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub(super) fn name(_pid: u32) -> Option<String> {
    None
}

/// Only the installed total is known on this system.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub(super) fn system() -> SystemMemory {
    SystemMemory {
        total: Some(super::physical_bytes()).filter(|bytes| *bytes > 0),
        ..SystemMemory::default()
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn this_process_and_the_device_report_their_memory() {
        let own = process(None);
        assert!(own.resident.is_some_and(|bytes| bytes > 0), "{own:?}");
        let by_pid = process(Some(std::process::id()));
        assert!(by_pid.resident.is_some(), "{by_pid:?}");
        assert!(name(std::process::id()).is_some_and(|name| !name.is_empty()));
        let system = system();
        assert!(
            system.total.is_some_and(|bytes| bytes >= 256 << 20),
            "{system:?}"
        );
        assert!(system.available.is_some(), "{system:?}");
    }

    #[test]
    fn a_process_that_does_not_exist_reports_nothing() {
        // PID numbers stop far below this on Linux.
        assert!(process(Some(u32::MAX)).is_empty());
        assert_eq!(name(u32::MAX), None);
    }
}
