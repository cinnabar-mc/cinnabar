//! A Landlock ruleset that handles every access right the kernel knows and grants none, so the
//! helper can open no file or directory, and, where the kernel supports it, bind or connect no
//! TCP socket and signal or reach no process outside itself.

use super::Lockdown;
use anyhow::{Result, bail, ensure};

/// `LANDLOCK_CREATE_RULESET_VERSION`: query the ABI instead of creating a ruleset.
const CREATE_RULESET_VERSION: u32 = 1;
/// The 13 filesystem rights of ABI 1.
const FS_ABI1: u64 = (1 << 13) - 1;
/// `LANDLOCK_ACCESS_FS_REFER` (ABI 2).
const FS_REFER: u64 = 1 << 13;
/// `LANDLOCK_ACCESS_FS_TRUNCATE` (ABI 3).
const FS_TRUNCATE: u64 = 1 << 14;
/// `LANDLOCK_ACCESS_FS_IOCTL_DEV` (ABI 5).
const FS_IOCTL_DEV: u64 = 1 << 15;
/// `LANDLOCK_ACCESS_NET_BIND_TCP | LANDLOCK_ACCESS_NET_CONNECT_TCP` (ABI 4).
const NET_TCP: u64 = 0b11;
/// `LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET | LANDLOCK_SCOPE_SIGNAL` (ABI 6).
const SCOPES: u64 = 0b11;

/// `struct landlock_ruleset_attr`; older kernels read only its leading fields.
#[repr(C)]
struct RulesetAttr {
    handled_access_fs: u64,
    handled_access_net: u64,
    scoped: u64,
}

/// The ruleset of ABI `abi` and the size of the attribute prefix that ABI reads.
fn ruleset(abi: u32) -> (RulesetAttr, usize) {
    let mut fs = FS_ABI1;
    if abi >= 2 {
        fs |= FS_REFER;
    }
    if abi >= 3 {
        fs |= FS_TRUNCATE;
    }
    if abi >= 5 {
        fs |= FS_IOCTL_DEV;
    }
    let attr = RulesetAttr {
        handled_access_fs: fs,
        handled_access_net: if abi >= 4 { NET_TCP } else { 0 },
        scoped: if abi >= 6 { SCOPES } else { 0 },
    };
    let size = match abi {
        ..4 => 8,
        4 | 5 => 16,
        _ => 24,
    };
    (attr, size)
}

/// Restricts the calling thread, and the threads it creates, to no filesystem access.
pub(super) fn restrict() -> Result<Lockdown> {
    // SAFETY: the version query takes no attribute.
    let abi = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            std::ptr::null::<RulesetAttr>(),
            0usize,
            CREATE_RULESET_VERSION,
        )
    };
    if abi < 1 {
        bail!(
            "unavailable ({}); the helper cannot deny itself the filesystem",
            std::io::Error::last_os_error()
        );
    }
    let abi = abi as u32;
    let (attr, size) = ruleset(abi);
    // SAFETY: `attr` outlives the call and `size` never exceeds it.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            &attr as *const RulesetAttr,
            size,
            0u32,
        )
    };
    ensure!(
        fd >= 0,
        "ruleset refused: {}",
        std::io::Error::last_os_error()
    );
    let fd = fd as libc::c_int;
    // SAFETY: plain system calls on the ruleset descriptor, which is closed afterwards.
    let status = unsafe {
        libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
        let status = libc::syscall(libc::SYS_landlock_restrict_self, fd, 0u32);
        libc::close(fd);
        status
    };
    ensure!(
        status == 0,
        "restriction refused: {}",
        std::io::Error::last_os_error()
    );
    Ok(Lockdown {
        landlock_abi: abi,
        network_denied: abi >= 4,
        scoped: abi >= 6,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_abi_handles_every_right_it_knows_in_the_prefix_it_reads() {
        let (first, size) = ruleset(1);
        assert_eq!((first.handled_access_fs, size), (FS_ABI1, 8));
        let (fourth, size) = ruleset(4);
        assert_eq!(fourth.handled_access_fs, FS_ABI1 | FS_REFER | FS_TRUNCATE);
        assert_eq!((fourth.handled_access_net, size), (NET_TCP, 16));
        let (sixth, size) = ruleset(6);
        assert_eq!(sixth.handled_access_fs, (1 << 16) - 1);
        assert_eq!((sixth.scoped, size), (SCOPES, 24));
        assert_eq!(size_of::<RulesetAttr>(), 24);
    }
}
