//! Physical device memory shared by global and server pack selection.

/// Reads physical RAM without using current free memory or process memory pressure.
pub fn physical_bytes() -> u64 {
    static BYTES: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *BYTES.get_or_init(|| {
        let result = read_physical_bytes();
        if result.is_none() {
            tracing::warn!(
                "physical memory unavailable; resource packs use the lowest automatic tier"
            );
        }
        result.unwrap_or(0)
    })
}

/// The smallest soft memory limit the client gives one `bedrock-core` process.
const CORE_MEMORY_LIMIT_FLOOR: u64 = 128 << 20;
/// The largest soft memory limit the client gives one `bedrock-core` process.
const CORE_MEMORY_LIMIT_CEILING: u64 = 512 << 20;
/// The share of physical memory, as a divisor, one `bedrock-core` process may aim to stay under.
const CORE_MEMORY_SHARE: u64 = 16;

/// The soft memory limit, in bytes, for one `bedrock-core` process on a device with
/// `physical_bytes` of RAM: a sixteenth of it in whole MiB, kept between 128 and 512 MiB, so a
/// phone's core collects garbage well before a desktop's would. `None` when RAM is unknown, which
/// leaves the core its built-in limit.
pub fn core_memory_limit(physical_bytes: u64) -> Option<u64> {
    const MIB: u64 = 1 << 20;
    (physical_bytes != 0).then(|| {
        (physical_bytes / CORE_MEMORY_SHARE / MIB * MIB)
            .clamp(CORE_MEMORY_LIMIT_FLOOR, CORE_MEMORY_LIMIT_CEILING)
    })
}

/// Reads the kernel's byte-valued `hw.memsize` property, which iOS also provides.
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn read_physical_bytes() -> Option<u64> {
    use std::ffi::{c_char, c_int, c_void};
    unsafe extern "C" {
        fn sysctlbyname(
            name: *const c_char,
            old: *mut c_void,
            old_length: *mut usize,
            new: *mut c_void,
            new_length: usize,
        ) -> c_int;
    }
    let mut bytes = 0u64;
    let mut length = size_of::<u64>();
    // SAFETY: the name is NUL-terminated and `bytes` has the `length` the kernel may write.
    let result = unsafe {
        sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&raw mut bytes).cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    (result == 0 && length == size_of::<u64>()).then_some(bytes)
}

/// Android shares the Linux kernel's `/proc/meminfo`, which apps may read.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn read_physical_bytes() -> Option<u64> {
    meminfo_total_bytes(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

/// `MemTotal` from `/proc/meminfo`, which the kernel reports in KiB.
#[cfg(any(target_os = "linux", target_os = "android", test))]
fn meminfo_total_bytes(meminfo: &str) -> Option<u64> {
    let line = meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))?;
    line.split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?
        .checked_mul(1024)
}

#[cfg(target_os = "windows")]
fn read_physical_bytes() -> Option<u64> {
    crate::desktop::windows::physical_memory()
}

/// Unsupported platforms retain the lowest automatic tier until a native reader exists.
#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    target_os = "windows"
)))]
fn read_physical_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo_total_is_read_in_kib() {
        let meminfo = "MemFree:  1000 kB\nMemTotal:       16318060 kB\nSwapTotal: 0 kB\n";
        assert_eq!(meminfo_total_bytes(meminfo), Some(16_318_060 * 1024));
        assert_eq!(meminfo_total_bytes("MemFree: 1 kB\n"), None);
        assert_eq!(meminfo_total_bytes("MemTotal: lots kB\n"), None);
    }

    #[test]
    fn the_core_limit_is_a_sixteenth_of_device_memory_within_its_bounds() {
        const MIB: u64 = 1 << 20;
        // Reported RAM in MiB (phones report less than their nominal size) and the limit in MiB.
        for (physical, limit) in [
            (1_843, 128),
            (2_048, 128),
            (2_765, 172),
            (3_789, 236),
            (5_365, 335),
            (7_680, 480),
            (8_192, 512),
            (65_536, 512),
        ] {
            assert_eq!(
                core_memory_limit(physical * MIB),
                Some(limit * MIB),
                "{physical} MiB"
            );
        }
        assert_eq!(core_memory_limit(5_365 * MIB + 4_096), Some(335 * MIB));
        assert_eq!(core_memory_limit(1), Some(128 * MIB));
        assert_eq!(core_memory_limit(0), None, "unknown memory sets no limit");
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    #[test]
    fn native_reader_reports_installed_memory() {
        let bytes = read_physical_bytes().expect("physical memory");
        assert!(bytes >= 256 << 20, "{bytes}");
    }
}
