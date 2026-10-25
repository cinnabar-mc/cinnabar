//! The restricted helper launch, on Linux. The parent clears the environment and, between
//! `fork` and `exec`, closes every descriptor but the three stdio pipes, sets resource limits
//! and forbids privilege gain; the helper then makes itself undumpable, audits what it
//! inherited and, before it reads the component, denies itself the filesystem and the network
//! with Landlock and process creation, executables, sockets and kernel attack surface with a
//! seccomp filter. Compilation of the component happens after this, so the compiler runs
//! contained too. Other platforms have no restricted launch and refuse it.

use anyhow::{Context, Result, ensure};
use std::process::Command;

#[cfg(target_os = "linux")]
mod landlock;
#[cfg(target_os = "linux")]
mod seccomp;

/// The helper subcommand that locks itself down before serving.
pub const RESTRICTED_COMMAND: &str = "restricted-server-helper";
/// The helper's private writable memory (`RLIMIT_DATA`): the guest's linear memory, the
/// compiler's working set and the runtime, with headroom.
pub const DATA_LIMIT_BYTES: u64 = 1 << 30;
/// Descriptors the helper may hold (`RLIMIT_NOFILE`): stdio and the runtime's own.
pub const DESCRIPTOR_LIMIT: u64 = 64;

/// What [`lock_down`] applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lockdown {
    /// The Landlock ABI the filesystem and network denial used.
    pub landlock_abi: u32,
    /// Whether Landlock also denied TCP bind and connect (ABI 4 and later).
    pub network_denied: bool,
    /// Whether Landlock also scoped signals and abstract sockets to the helper (ABI 6 and
    /// later).
    pub scoped: bool,
}

/// Makes `command` launch contained: no environment, only the stdio descriptors, the
/// helper's resource limits, no privilege gain, and death with the parent. The kernel sends
/// the death signal when the thread that spawns the command exits, so that thread must outlive
/// the helper's use; the supervisor spawns from the thread that owns the helper's pipes.
#[cfg(target_os = "linux")]
pub fn contain(command: &mut Command) {
    contain_expecting(command, std::process::id() as libc::pid_t);
}

/// [`contain`], with the child refusing to run unless its parent is still process `parent`
/// once its death signal is armed: a parent that died before then would never send it.
#[cfg(target_os = "linux")]
fn contain_expecting(command: &mut Command, parent: libc::pid_t) {
    use std::os::unix::process::CommandExt;
    command.env_clear();
    // SAFETY: the closure makes only async-signal-safe raw system calls.
    unsafe { command.pre_exec(move || prepare_child(parent)) };
}

/// Restricted launch is Linux-only; elsewhere the command is left as is and
/// [`lock_down`] refuses.
#[cfg(not(target_os = "linux"))]
pub fn contain(command: &mut Command) {
    command.env_clear();
}

/// The resource limits a contained helper runs under.
#[cfg(target_os = "linux")]
pub const LIMITS: [(libc::__rlimit_resource_t, u64); 7] = [
    (libc::RLIMIT_DATA, DATA_LIMIT_BYTES),
    (libc::RLIMIT_NOFILE, DESCRIPTOR_LIMIT),
    // The runtime sizes anonymous memory files for linear memory images; Landlock already
    // keeps the helper from opening any other file.
    (libc::RLIMIT_FSIZE, DATA_LIMIT_BYTES),
    (libc::RLIMIT_CORE, 0),
    (libc::RLIMIT_MEMLOCK, 0),
    (libc::RLIMIT_MSGQUEUE, 0),
    (libc::RLIMIT_RTPRIO, 0),
];

/// Runs in the child between `fork` and `exec`: only raw system calls, no allocation. Fails
/// with `ESRCH` when the child's parent is no longer `parent`.
#[cfg(target_os = "linux")]
fn prepare_child(parent: libc::pid_t) -> std::io::Result<()> {
    let check = |status: libc::c_int| {
        if status == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    };
    // SAFETY: plain system calls on constant arguments and fully initialized structs.
    unsafe {
        check(libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0))?;
        if libc::getppid() != parent {
            return Err(std::io::Error::from_raw_os_error(libc::ESRCH));
        }
        check(libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0))?;
        // Covers the copy of the parent's memory until `exec`, which resets the flag for an
        // ordinary executable; [`lock_down`] sets it again in the helper.
        check(libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0))?;
        if libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32) != 0 {
            // `close_range` is missing (kernels before 5.9) or refused by an outer filter: close
            // every descriptor the current hard limit can address, up to the kernel's default
            // ceiling. One numbered higher survives, and the helper's audit then refuses to
            // serve.
            let mut limit = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            let ceiling = if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) == 0 {
                limit.rlim_max.min(FALLBACK_CLOSE_CEILING)
            } else {
                FALLBACK_CLOSE_CEILING
            };
            for fd in 3..ceiling as libc::c_int {
                libc::close(fd);
            }
        }
        for (resource, value) in LIMITS {
            let limit = libc::rlimit {
                rlim_cur: value as libc::rlim_t,
                rlim_max: value as libc::rlim_t,
            };
            check(libc::setrlimit(resource, &limit))?;
        }
    }
    Ok(())
}

/// The descriptor the fallback closure before `exec` stops at when `close_range` cannot run:
/// the kernel's default `fs.nr_open`, which bounds the hard descriptor limit unless raised.
#[cfg(target_os = "linux")]
const FALLBACK_CLOSE_CEILING: libc::rlim_t = 1 << 20;

/// The open descriptors above stdio this process holds, listed from `/proc/self/fd` so that
/// none escapes however high its number or however low the descriptor limit. Fails when the
/// listing is unavailable, so an audit built on it fails closed.
#[cfg(target_os = "linux")]
pub fn inherited_descriptors() -> Result<Vec<i32>> {
    const LISTING: &str = "/proc/self/fd";
    let mut listed = Vec::new();
    for entry in std::fs::read_dir(LISTING).with_context(|| format!("cannot list {LISTING}"))? {
        let name = entry
            .with_context(|| format!("cannot list {LISTING}"))?
            .file_name();
        let fd = name.to_str().and_then(|name| name.parse::<i32>().ok());
        listed.push(fd.with_context(|| format!("{LISTING} lists {name:?}"))?);
    }
    // The listing's own descriptor is closed by now and fails the check.
    // SAFETY: F_GETFD only queries a descriptor's flags.
    Ok(listed
        .into_iter()
        .filter(|&fd| fd > 2 && unsafe { libc::fcntl(fd, libc::F_GETFD) } != -1)
        .collect())
}

/// Audits what the helper inherited and denies it everything it does not need. It must run on
/// the main thread before any other thread starts, because Landlock restricts the calling
/// thread and the threads it then creates.
#[cfg(target_os = "linux")]
pub fn lock_down() -> Result<Lockdown> {
    // SAFETY: plain prctl calls on constant arguments.
    ensure!(
        unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } == 0
            && unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) } == 0,
        "the helper cannot make itself undumpable"
    );
    let inherited = inherited_descriptors().context("the helper cannot audit its descriptors")?;
    ensure!(
        inherited.is_empty(),
        "the helper inherited descriptors {inherited:?}"
    );
    ensure!(
        std::env::vars_os().next().is_none(),
        "the helper inherited an environment"
    );
    for (resource, value) in LIMITS {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: getrlimit fills the struct.
        ensure!(
            unsafe { libc::getrlimit(resource, &mut limit) } == 0 && limit.rlim_max <= value,
            "the helper runs without its resource limits"
        );
    }
    // SAFETY: plain prctl queries.
    ensure!(
        unsafe { libc::prctl(libc::PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) } == 1,
        "the helper may gain privileges"
    );
    let lockdown = landlock::restrict().context("Landlock")?;
    seccomp::install().context("seccomp")?;
    Ok(lockdown)
}

/// Restricted helpers are Linux-only.
#[cfg(not(target_os = "linux"))]
pub fn lock_down() -> Result<Lockdown> {
    anyhow::bail!("restricted server helpers are not implemented on this platform")
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// A child that finds another parent once its death signal is armed, as when the one that
    /// launched it died first, refuses to run; under its real parent it runs.
    #[test]
    fn a_child_orphaned_before_its_death_signal_refuses_to_run() {
        let launch = |parent| {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "no-such-test"]);
            contain_expecting(&mut command, parent);
            command.output()
        };
        let orphaned = launch(libc::pid_t::MAX).unwrap_err();
        assert_eq!(orphaned.raw_os_error(), Some(libc::ESRCH));
        let output = launch(std::process::id() as libc::pid_t).unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}
