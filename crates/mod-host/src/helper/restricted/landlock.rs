//! A Landlock ruleset that handles every access right the kernel knows and grants none, so the
//! helper can open no file or directory, and, where the kernel supports it, bind or connect no
//! TCP or UDP socket and signal or reach no process outside itself. The rights are probed from
//! the running kernel, so those a newer ABI adds are denied too.

use super::Lockdown;
use anyhow::{Result, bail, ensure};

/// `LANDLOCK_CREATE_RULESET_VERSION`: query the ABI instead of creating a ruleset.
const CREATE_RULESET_VERSION: u32 = 1;
/// The 13 filesystem rights of ABI 1, which every Landlock kernel knows.
const FS_ABI1: u64 = (1 << 13) - 1;
/// `LANDLOCK_ACCESS_NET_BIND_TCP | LANDLOCK_ACCESS_NET_CONNECT_TCP` (ABI 4).
const NET_TCP: u64 = 0b11;
/// `LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET | LANDLOCK_SCOPE_SIGNAL` (ABI 6).
const SCOPES: u64 = 0b11;

/// `struct landlock_ruleset_attr` up to its last field of denials; older kernels read only its
/// leading fields, and the later fields only quiet logging.
#[repr(C)]
#[derive(Debug, Default)]
struct RulesetAttr {
    handled_access_fs: u64,
    handled_access_net: u64,
    scoped: u64,
}

/// A field of [`RulesetAttr`] that lists handled rights, in declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Fs,
    Net,
    Scoped,
}

impl Field {
    const ALL: [Field; 3] = [Field::Fs, Field::Net, Field::Scoped];

    /// The size of the attribute prefix that ends with this field.
    fn prefix(self) -> usize {
        (self as usize + 1) * size_of::<u64>()
    }
}

impl RulesetAttr {
    /// The rights this ruleset handles in `field`.
    fn field_mut(&mut self, field: Field) -> &mut u64 {
        match field {
            Field::Fs => &mut self.handled_access_fs,
            Field::Net => &mut self.handled_access_net,
            Field::Scoped => &mut self.scoped,
        }
    }
}

/// The ruleset handling every right `knows` reports, probed one bit of each field at a time so
/// that rights of newer ABIs are handled without a table to update, and the shortest attribute
/// prefix that carries them. Fails with the first error of `knows`.
fn ruleset(mut knows: impl FnMut(Field, u64) -> Result<bool>) -> Result<(RulesetAttr, usize)> {
    let mut attr = RulesetAttr::default();
    let mut size = Field::Fs.prefix();
    for field in Field::ALL {
        for bit in 0..u64::BITS {
            let right = 1u64 << bit;
            if knows(field, right)? {
                *attr.field_mut(field) |= right;
                size = size.max(field.prefix());
            }
        }
    }
    Ok((attr, size))
}

/// Creates a ruleset from the first `size` bytes of `attr` and returns its descriptor, or -1
/// with `errno` set.
fn create_ruleset(attr: &RulesetAttr, size: usize) -> libc::c_long {
    // SAFETY: `attr` outlives the call and `size` never exceeds it.
    unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            attr as *const RulesetAttr,
            size,
            0u32,
        )
    }
}

/// Whether the running kernel knows `right` in `field`: it refuses an unknown right with
/// `EINVAL` and an unknown field, set through a longer attribute than it reads, with `E2BIG`.
/// Any other refusal is an error, so a failing probe cannot leave a right unhandled.
fn kernel_knows(field: Field, right: u64) -> Result<bool> {
    let mut attr = RulesetAttr::default();
    *attr.field_mut(field) = right;
    let fd = create_ruleset(&attr, field.prefix());
    if fd >= 0 {
        // SAFETY: closing the probe ruleset this call created.
        unsafe { libc::close(fd as libc::c_int) };
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::EINVAL | libc::E2BIG) => Ok(false),
        _ => bail!("probing {field:?} right {right:#x} failed: {error}"),
    }
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
    let (attr, size) = ruleset(kernel_knows)?;
    ensure!(
        attr.handled_access_fs & FS_ABI1 == FS_ABI1,
        "the kernel reports ABI {abi} but not every filesystem right of ABI 1"
    );
    let fd = create_ruleset(&attr, size);
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
        network_denied: attr.handled_access_net & NET_TCP == NET_TCP,
        scoped: attr.scoped & SCOPES == SCOPES,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `LANDLOCK_ACCESS_FS_REFER` (ABI 2).
    const FS_REFER: u64 = 1 << 13;
    /// `LANDLOCK_ACCESS_FS_TRUNCATE` (ABI 3).
    const FS_TRUNCATE: u64 = 1 << 14;
    /// `LANDLOCK_ACCESS_FS_IOCTL_DEV` (ABI 5).
    const FS_IOCTL_DEV: u64 = 1 << 15;
    /// `LANDLOCK_ACCESS_FS_RESOLVE_UNIX` (ABI 9).
    const FS_RESOLVE_UNIX: u64 = 1 << 16;
    /// `LANDLOCK_ACCESS_NET_BIND_UDP | LANDLOCK_ACCESS_NET_CONNECT_SEND_UDP` (ABI 10).
    const NET_UDP: u64 = 0b1100;

    /// A kernel of Landlock ABI `abi`, answering probes as the real one does.
    fn kernel(abi: u32) -> impl FnMut(Field, u64) -> Result<bool> {
        let mut known = RulesetAttr {
            handled_access_fs: FS_ABI1,
            ..RulesetAttr::default()
        };
        for (since, field, rights) in [
            (2, Field::Fs, FS_REFER),
            (3, Field::Fs, FS_TRUNCATE),
            (4, Field::Net, NET_TCP),
            (5, Field::Fs, FS_IOCTL_DEV),
            (6, Field::Scoped, SCOPES),
            (9, Field::Fs, FS_RESOLVE_UNIX),
            (10, Field::Net, NET_UDP),
        ] {
            if abi >= since {
                *known.field_mut(field) |= rights;
            }
        }
        move |field, right| Ok(*known.field_mut(field) & right != 0)
    }

    #[test]
    fn each_abi_handles_every_right_it_knows_in_the_prefix_it_reads() {
        let (first, size) = ruleset(kernel(1)).unwrap();
        assert_eq!((first.handled_access_fs, size), (FS_ABI1, 8));
        let (fourth, size) = ruleset(kernel(4)).unwrap();
        assert_eq!(fourth.handled_access_fs, FS_ABI1 | FS_REFER | FS_TRUNCATE);
        assert_eq!((fourth.handled_access_net, size), (NET_TCP, 16));
        let (sixth, size) = ruleset(kernel(6)).unwrap();
        assert_eq!(sixth.handled_access_fs, (1 << 16) - 1);
        assert_eq!((sixth.scoped, size), (SCOPES, 24));
        assert_eq!(size_of::<RulesetAttr>(), 24);
    }

    /// Rights a newer ABI adds are handled too, so they are denied rather than left open.
    #[test]
    fn rights_of_newer_abis_are_handled() {
        let (tenth, size) = ruleset(kernel(10)).unwrap();
        assert_eq!(tenth.handled_access_fs, (1 << 17) - 1);
        assert_eq!(tenth.handled_access_net, NET_TCP | NET_UDP);
        assert_eq!((tenth.scoped, size), (SCOPES, 24));
    }

    /// A probe the kernel fails for another reason fails the ruleset instead of dropping the
    /// right.
    #[test]
    fn a_failing_probe_fails_the_ruleset() {
        let mut real = kernel(6);
        let failing = ruleset(|field, right| {
            if field == Field::Net {
                bail!("out of descriptors");
            }
            real(field, right)
        });
        assert!(failing.is_err());
    }
}
