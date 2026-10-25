//! The restricted helper launch, on Linux. The parent clears the environment, creates a
//! memory-limited cgroup for the helper and, between `fork` and `exec`, moves the child into it,
//! has `exec` close every descriptor but the three stdio pipes, sets resource limits and forbids
//! privilege gain; the helper then makes itself undumpable, audits what it inherited and, before it reads the component, denies itself the filesystem and the network
//! with Landlock and process creation, executables, sockets and kernel attack surface with a
//! seccomp filter. Compilation of the component happens after this, so the compiler runs
//! contained too. Other platforms have no restricted launch and refuse it.

use anyhow::{Context, Result, ensure};
use std::process::Command;

#[cfg(target_os = "linux")]
mod cgroup;
#[cfg(target_os = "linux")]
mod landlock;
#[cfg(target_os = "linux")]
mod seccomp;

/// The helper subcommand that locks itself down before serving.
pub const RESTRICTED_COMMAND: &str = "restricted-server-helper";
/// The helper's private writable memory (`RLIMIT_DATA`): the guest's linear memory, the
/// compiler's working set and the runtime, with headroom.
pub const DATA_LIMIT_BYTES: u64 = 1 << 30;
/// All the memory the helper may be charged for, its cgroup's `memory.max`: the private
/// memory of [`DATA_LIMIT_BYTES`] and room for the shared memory files of linear memory
/// images, its executable's page cache and kernel memory. Swap is closed.
pub const MEMORY_LIMIT_BYTES: u64 = DATA_LIMIT_BYTES + (1 << 29);
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

/// What a contained launch holds until its helper has exited: on Linux, the helper's cgroup,
/// which dropping removes.
#[derive(Debug)]
pub struct Containment {
    #[cfg(target_os = "linux")]
    cgroup: cgroup::Cgroup,
}

#[cfg(target_os = "linux")]
impl Containment {
    /// The directory of the helper's cgroup.
    pub fn cgroup(&self) -> &std::path::Path {
        self.cgroup.path()
    }
}

/// Makes `command` launch contained: no environment, the root as working directory, its own
/// cgroup limited to [`MEMORY_LIMIT_BYTES`] without swap, only the stdio descriptors, the
/// helper's resource limits, no privilege gain, and death with the parent. The kernel sends
/// the death signal when the thread that spawns the command exits, so that thread must outlive
/// the helper's use; the supervisor spawns from the thread that owns the helper's pipes. The
/// returned containment must outlive the helper's process. Fails, so the launch fails closed,
/// when no delegated cgroup can limit the helper's memory.
#[cfg(target_os = "linux")]
pub fn contain(command: &mut Command) -> Result<Containment> {
    let cgroup = cgroup::Cgroup::create().context("the helper cannot be memory-limited")?;
    let procs = cgroup
        .procs()
        .context("the helper cannot be memory-limited")?;
    contain_expecting(command, std::process::id() as libc::pid_t, Some(procs));
    Ok(Containment { cgroup })
}

/// [`contain`] without its cgroup, with the child joining the cgroup whose `cgroup.procs` is
/// open as `procs`, if any, and refusing to run unless its parent is still process `parent`
/// once its death signal is armed: a parent that died before then would never send it.
#[cfg(target_os = "linux")]
fn contain_expecting(command: &mut Command, parent: libc::pid_t, procs: Option<std::fs::File>) {
    use std::os::{fd::AsRawFd, unix::process::CommandExt};
    command.env_clear().current_dir("/");
    // SAFETY: the closure makes only async-signal-safe raw system calls, on a descriptor it
    // owns.
    unsafe {
        command.pre_exec(move || prepare_child(parent, procs.as_ref().map(AsRawFd::as_raw_fd)))
    };
}

/// Restricted launch is Linux-only; elsewhere the command is left as is and
/// [`lock_down`] refuses.
#[cfg(not(target_os = "linux"))]
pub fn contain(command: &mut Command) -> Result<Containment> {
    command.env_clear();
    Ok(Containment {})
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

/// Runs in the child between `fork` and `exec`: only raw system calls, no allocation. Moves
/// the child into the cgroup whose `cgroup.procs` is open as `procs`, if any. Fails with
/// `ESRCH` when the child's parent is no longer `parent`.
#[cfg(target_os = "linux")]
fn prepare_child(parent: libc::pid_t, procs: Option<libc::c_int>) -> std::io::Result<()> {
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
        // Writing 0 to a cgroup's process list moves the writer, before it runs any helper code.
        if let Some(procs) = procs
            && libc::write(procs, b"0".as_ptr().cast(), 1) != 1
        {
            return Err(std::io::Error::last_os_error());
        }
        // Every descriptor above stdio becomes close-on-exec rather than closed at once, so the
        // pipe through which `spawn` learns of a failure below stays open until `exec`.
        if libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, CLOSE_RANGE_CLOEXEC) != 0 {
            // `close_range` or its flag is missing (kernels before 5.11) or refused by an outer
            // filter: mark every descriptor the current hard limit can address, up to the
            // kernel's default ceiling. One numbered higher survives, and the helper's audit
            // then refuses to serve.
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
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
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

/// `CLOSE_RANGE_CLOEXEC` (`linux/close_range.h`), which the C library crate lacks on Linux.
#[cfg(target_os = "linux")]
const CLOSE_RANGE_CLOEXEC: u32 = 1 << 2;

/// The descriptor the fallback marking before `exec` stops at when `close_range` cannot run:
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

/// The kernel feature or delegation that restricted helpers need and this system lacks, named with the
/// reason, or none. A lockdown fails without it; tests that lock a helper down skip with it.
#[cfg(target_os = "linux")]
pub fn missing_feature() -> Option<String> {
    if let Err(error) = landlock::abi() {
        return Some(format!("kernel feature Landlock ({error})"));
    }
    cgroup::Cgroup::create()
        .err()
        .map(|error| format!("a delegated cgroup v2 memory controller ({error:#})"))
}

/// Audits what the helper inherited and denies it everything it does not need. It must run on
/// the main thread before any other thread starts, because Landlock restricts the calling
/// thread and the threads it then creates; [`ensure_single_threaded`] checks that.
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
    cgroup::audit()?;
    let lockdown = landlock::restrict().context("Landlock")?;
    seccomp::install().context("seccomp")?;
    Ok(lockdown)
}

/// Fails unless the calling process runs one thread, as listed in `/proc/self/task`, or when
/// that listing is unavailable. Landlock restricts only the calling thread and the threads it
/// creates afterwards, so the helper checks this before [`lock_down`].
#[cfg(target_os = "linux")]
pub fn ensure_single_threaded() -> Result<()> {
    const LISTING: &str = "/proc/self/task";
    let threads = std::fs::read_dir(LISTING)
        .with_context(|| format!("cannot list {LISTING}"))?
        .count();
    ensure!(
        threads == 1,
        "the helper runs {threads} threads before its lockdown"
    );
    Ok(())
}

/// Restricted helpers are Linux-only.
#[cfg(not(target_os = "linux"))]
pub fn ensure_single_threaded() -> Result<()> {
    anyhow::bail!("restricted server helpers are not implemented on this platform")
}

/// Restricted helpers are Linux-only.
#[cfg(not(target_os = "linux"))]
pub fn lock_down() -> Result<Lockdown> {
    anyhow::bail!("restricted server helpers are not implemented on this platform")
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// Set for the launcher process of
    /// [`a_containment_step_failing_after_descriptors_are_marked_fails_the_spawn`].
    const LAUNCHER: &str = "RESTRICTED_LAUNCHER";

    /// Does nothing in an ordinary run. As the launcher, lowers its hard file size limit just
    /// below the helper's, so that the containment step setting it, which runs after the
    /// descriptors are marked, fails, and prints whether the spawn reported that.
    #[test]
    fn launcher_with_a_lowered_file_size_limit() {
        if std::env::var_os(LAUNCHER).is_none() {
            return;
        }
        let lowered = (DATA_LIMIT_BYTES - 1) as libc::rlim_t;
        let limit = libc::rlimit {
            rlim_cur: lowered,
            rlim_max: lowered,
        };
        // SAFETY: setrlimit reads the initialized struct.
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &limit) }, 0);
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "no-such-test"]);
        contain_expecting(&mut command, std::process::id() as libc::pid_t, None);
        match command.output() {
            Err(error) => println!("LAUNCH refused {}", error.raw_os_error().unwrap_or(-1)),
            Ok(output) => println!("LAUNCH ran {}", output.status),
        }
    }

    /// A containment step that fails after the inherited descriptors are dealt with still
    /// fails the spawn with its error, rather than the child dying unreported.
    #[test]
    fn a_containment_step_failing_after_descriptors_are_marked_fails_the_spawn() {
        // SAFETY: geteuid only reads the caller's identity.
        if unsafe { libc::geteuid() } == 0 {
            eprintln!("skipping: running as root, which may raise its hard limits");
            return;
        }
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "helper::restricted::tests::launcher_with_a_lowered_file_size_limit",
                "--nocapture",
            ])
            .env(LAUNCHER, "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(&format!("LAUNCH refused {}", libc::EPERM)),
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A process running a second thread refuses to lock down, since Landlock would leave that
    /// thread unrestricted.
    #[test]
    fn a_second_thread_fails_the_single_thread_check() {
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        let (started, running) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            started.send(()).unwrap();
            let _ = stopped.recv();
        });
        running.recv().unwrap();
        let error = ensure_single_threaded().unwrap_err().to_string();
        drop(stop);
        thread.join().unwrap();
        assert!(error.contains("threads before its lockdown"), "{error}");
    }

    /// A child that finds another parent once its death signal is armed, as when the one that
    /// launched it died first, refuses to run; under its real parent it runs.
    #[test]
    fn a_child_orphaned_before_its_death_signal_refuses_to_run() {
        let launch = |parent| {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "no-such-test"]);
            contain_expecting(&mut command, parent, None);
            command.output()
        };
        let orphaned = launch(libc::pid_t::MAX).unwrap_err();
        assert_eq!(orphaned.raw_os_error(), Some(libc::ESRCH));
        let output = launch(std::process::id() as libc::pid_t).unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}
