//! Reads the kernel's memory accounting for this process and its helpers. Linux and Android
//! expose it under `/proc`; other systems report nothing.

use super::snapshot::{ProcessMemory, SystemMemory};

/// Memory of process `pid`, or of this process for `None`. A process of the same user is
/// readable; one that has exited, or a file the system hides, leaves its figures unknown.
///
/// Only a `detailed` reading includes the proportional size: `smaps_rollup` walks every mapping
/// under the process's address-space lock, which stalls its threads' `mmap` calls on a large
/// process, so frequent readings take resident and swapped sizes from the kernel's counters in
/// `status` instead.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn process(pid: Option<u32>, detailed: bool) -> ProcessMemory {
    let dir = pid.map_or_else(|| "/proc/self".to_owned(), |pid| format!("/proc/{pid}"));
    let source = if detailed { "smaps_rollup" } else { "status" };
    let text = std::fs::read_to_string(format!("{dir}/{source}")).unwrap_or_default();
    let memory = if detailed {
        ProcessMemory::from_smaps_rollup(&text)
    } else {
        ProcessMemory::from_status(&text)
    };
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
pub(super) fn process(_pid: Option<u32>, _detailed: bool) -> ProcessMemory {
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
        for detailed in [true, false] {
            let own = process(None, detailed);
            assert!(own.resident.is_some_and(|bytes| bytes > 0), "{own:?}");
            // Kernels before 4.14 have no rollup; the resident size then comes from `statm`.
            let rollup = std::path::Path::new("/proc/self/smaps_rollup").exists();
            assert_eq!(own.proportional.is_some(), detailed && rollup, "{own:?}");
        }
        let by_pid = process(Some(std::process::id()), false);
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
        assert!(process(Some(u32::MAX), true).is_empty());
        assert!(process(Some(u32::MAX), false).is_empty());
        assert_eq!(name(u32::MAX), None);
    }
}
