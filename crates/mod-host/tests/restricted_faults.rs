//! Fault injection against the restricted helper's lockdown on Linux. Each probe re-runs this
//! test binary as a child launched exactly as the restricted helper is (contained before
//! `exec`), which locks itself down and then attempts one forbidden operation and reports
//! whether the kernel refused it.

#![cfg(target_os = "linux")]

use mod_host::helper::restricted::{
    DATA_LIMIT_BYTES, DESCRIPTOR_LIMIT, LIMITS, MEMORY_LIMIT_BYTES, contain, inherited_descriptors,
    lock_down, missing_feature,
};
use std::{
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::Command,
};

/// The probe child's argument prefix: libtest takes it as one more name filter.
const PROBE: &str = "probe-op:";

/// Whether this system can lock a helper down. Otherwise it prints the missing feature, and
/// the calling test skips.
fn lockdown_supported() -> bool {
    match missing_feature() {
        None => true,
        Some(missing) => {
            eprintln!("skipping: missing {missing}");
            false
        }
    }
}

/// Runs `op` in a contained child that locks itself down first, and returns its report.
fn probe(op: &str) -> String {
    probe_with(op, |command| contain(command).unwrap())
}

/// Runs `op` in a child that `launch` prepares and that locks itself down first, and returns
/// its report, if any, and its output. Whatever `launch` returns is held until the child has
/// exited and is returned too.
fn run_probe<G>(
    op: &str,
    launch: impl FnOnce(&mut Command) -> G,
) -> (Option<String>, std::process::Output, G) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "probe_child",
        "--nocapture",
        "--test-threads=1",
        &format!("{PROBE}{op}"),
    ]);
    let held = launch(&mut command);
    let output = command.output().unwrap();
    let report = String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| {
            line.split_once("PROBE ")
                .map(|(_, report)| report.to_owned())
        });
    (report, output, held)
}

/// Runs `op` in a child that `launch` prepares and that locks itself down first, and returns
/// its report.
fn probe_with<G>(op: &str, launch: impl FnOnce(&mut Command) -> G) -> String {
    let (report, output, held) = run_probe(op, launch);
    drop(held);
    let stdout = String::from_utf8_lossy(&output.stdout);
    report.unwrap_or_else(|| {
        panic!(
            "probe {op} reported nothing ({}):\n{stdout}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// The probe side: does nothing in an ordinary run, otherwise locks down, runs the named
/// operation and prints `PROBE <outcome>`.
#[test]
fn probe_child() {
    let Some(op) = std::env::args().find_map(|arg| arg.strip_prefix(PROBE).map(str::to_owned))
    else {
        return;
    };
    let report = match lock_down() {
        Err(error) => format!("lockdown-failed {error:#}"),
        Ok(_) => run(&op),
    };
    println!("PROBE {report}");
}

/// `allowed` when `result` succeeded, else `denied <errno>`.
fn outcome(result: std::io::Result<()>) -> String {
    match result {
        Ok(()) => "allowed".into(),
        Err(error) => format!("denied {}", error.raw_os_error().unwrap_or(-1)),
    }
}

/// A raw call's status as an outcome.
fn status(value: i64) -> String {
    outcome(if value < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    })
}

/// Reads the descriptor limit of process `pid` (0 is the caller) with `prlimit`, changing
/// nothing.
fn prlimit_of(pid: libc::pid_t) -> String {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: a query that only fills `limit`.
    status(i64::from(unsafe {
        libc::prlimit(pid, libc::RLIMIT_NOFILE, std::ptr::null(), &mut limit)
    }))
}

/// Sets the priority of process `who` (0 is the caller) to the value it already has, changing
/// nothing.
fn renice(who: libc::id_t) -> String {
    // SAFETY: plain system calls on scalar arguments.
    status(i64::from(unsafe {
        let current = libc::getpriority(libc::PRIO_PROCESS, who);
        libc::setpriority(libc::PRIO_PROCESS, who, current)
    }))
}

/// A path as a C string for raw calls.
fn c_path(path: &str) -> std::ffi::CString {
    std::ffi::CString::new(path).unwrap()
}

/// Runs `change` on a fresh anonymous memory file, as the runtime holds for its memory images.
fn on_memory_file(change: impl FnOnce(libc::c_int) -> libc::c_int) -> String {
    // SAFETY: creates a descriptor this probe owns and closes it.
    unsafe {
        let fd = libc::memfd_create(c"probe".as_ptr(), libc::MFD_CLOEXEC);
        if fd < 0 {
            return format!("memfd-failed {}", std::io::Error::last_os_error());
        }
        let report = status(i64::from(change(fd)));
        libc::close(fd);
        report
    }
}

/// Writes `total` bytes into anonymous memory files, which `RLIMIT_DATA` does not count, each
/// kept below the file size limit, and reports `allowed` if every byte was written.
fn fill_memory_files(total: u64) -> String {
    const CHUNK: usize = 1 << 20;
    let chunk = vec![0x5a_u8; CHUNK];
    let per_file = DATA_LIMIT_BYTES / 2;
    let mut files = Vec::new();
    let mut written = 0;
    while written < total {
        if written % per_file == 0 {
            // SAFETY: creates a descriptor this probe owns until it exits.
            let fd = unsafe { libc::memfd_create(c"probe".as_ptr(), libc::MFD_CLOEXEC) };
            if fd < 0 {
                return status(-1);
            }
            files.push(fd);
        }
        let fd = *files.last().unwrap();
        // SAFETY: writes from a buffer of `CHUNK` initialized bytes.
        if unsafe { libc::write(fd, chunk.as_ptr().cast(), CHUNK) } != CHUNK as isize {
            return status(-1);
        }
        written += CHUNK as u64;
    }
    "allowed".into()
}

/// Attempts one operation the lockdown must refuse, or one it must leave. An operation on a
/// file the test owns names it after `=`.
fn run(op: &str) -> String {
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let (op, target) = op.split_once('=').unwrap_or((op, ""));
    match op {
        "read-file" => outcome(std::fs::read(manifest).map(drop)),
        "list-directory" => outcome(std::fs::read_dir("/").map(drop)),
        "write-file" => outcome(std::fs::write(std::env::temp_dir().join("probe"), b"x")),
        "tcp-connect" => outcome(std::net::TcpStream::connect("127.0.0.1:9").map(drop)),
        "udp-bind" => outcome(std::net::UdpSocket::bind("127.0.0.1:0").map(drop)),
        "unix-socket" => outcome(std::os::unix::net::UnixDatagram::unbound().map(drop)),
        "spawn" => outcome(
            Command::new("/bin/sh")
                .arg("-c")
                .arg("true")
                .status()
                .map(drop),
        ),
        // SAFETY: the child of a successful fork exits at once.
        "fork" => {
            let pid = unsafe { libc::fork() };
            if pid == 0 {
                unsafe { libc::_exit(0) };
            }
            status(i64::from(pid))
        }
        "ptrace" => status(unsafe { libc::ptrace(libc::PTRACE_TRACEME, 0, 0, 0) }),
        "unshare" => status(i64::from(unsafe { libc::unshare(libc::CLONE_NEWUSER) })),
        "memory" => {
            // SAFETY: an anonymous mapping, unmapped if it is granted.
            let size = (DATA_LIMIT_BYTES + (64 << 20)) as usize;
            let mapping = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    size,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                )
            };
            if mapping == libc::MAP_FAILED {
                status(-1)
            } else {
                unsafe { libc::munmap(mapping, size) };
                "allowed".into()
            }
        }
        "thread" => outcome(
            std::thread::spawn(|| 1 + 1)
                .join()
                .map(drop)
                .map_err(|_| std::io::Error::other("panicked")),
        ),
        "limits" => {
            let mut limit = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) };
            format!("nofile {}", limit.rlim_max)
        }
        "prlimit-parent" => prlimit_of(unsafe { libc::getppid() }),
        "prlimit-self" => prlimit_of(0),
        "prlimit-own-pid" => prlimit_of(std::process::id() as libc::pid_t),
        // SAFETY (signal and process probes): plain system calls on scalar arguments; the only
        // signal sent is SIGCONT, which a running process ignores.
        "signal-parent-check" => status(i64::from(unsafe { libc::kill(libc::getppid(), 0) })),
        "signal-parent" => status(i64::from(unsafe {
            libc::kill(libc::getppid(), libc::SIGCONT)
        })),
        "signal-group" => status(i64::from(unsafe { libc::kill(0, libc::SIGCONT) })),
        "tgkill-parent" => status(unsafe {
            let parent = libc::getppid();
            libc::syscall(libc::SYS_tgkill, parent, parent, libc::SIGCONT)
        }),
        "sigio-parent" => status(i64::from(unsafe {
            libc::fcntl(1, libc::F_SETOWN, libc::getppid())
        })),
        "pidfd-parent" => {
            status(unsafe { libc::syscall(libc::SYS_pidfd_open, libc::getppid(), 0u32) })
        }
        "renice-parent" => renice(unsafe { libc::getppid() } as libc::id_t),
        "signal-self" => status(i64::from(unsafe { libc::kill(libc::getpid(), 0) })),
        "tgkill-self" => {
            status(unsafe { libc::syscall(libc::SYS_tgkill, libc::getpid(), libc::gettid(), 0) })
        }
        "renice-self" => renice(0),
        "dumpable" => format!("dumpable {}", unsafe {
            libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0)
        }),
        // SAFETY (file attribute probes): raw calls on a NUL-terminated path the test owns, or
        // on a memory file the probe owns.
        "chmod" => outcome(std::fs::set_permissions(
            target,
            std::os::unix::fs::PermissionsExt::from_mode(0o600),
        )),
        "chown" => outcome(std::os::unix::fs::chown(
            target,
            Some(unsafe { libc::getuid() }),
            Some(unsafe { libc::getgid() }),
        )),
        "utimes" => status(i64::from(unsafe {
            libc::utimensat(libc::AT_FDCWD, c_path(target).as_ptr(), std::ptr::null(), 0)
        })),
        "setxattr" => status(i64::from(unsafe {
            libc::setxattr(
                c_path(target).as_ptr(),
                c"user.probe".as_ptr(),
                c"x".as_ptr().cast(),
                1,
                0,
            )
        })),
        "removexattr" => status(i64::from(unsafe {
            libc::removexattr(c_path(target).as_ptr(), c"user.probe".as_ptr())
        })),
        "truncate" => status(i64::from(unsafe {
            libc::truncate(c_path(target).as_ptr(), 0)
        })),
        "ftruncate-memfd" => on_memory_file(|fd| unsafe { libc::ftruncate(fd, 4096) }),
        "fchmod-memfd" => on_memory_file(|fd| unsafe { libc::fchmod(fd, 0o600) }),
        "fchown-memfd" => {
            on_memory_file(|fd| unsafe { libc::fchown(fd, libc::getuid(), libc::getgid()) })
        }
        "fsetxattr-memfd" => on_memory_file(|fd| unsafe {
            libc::fsetxattr(fd, c"user.probe".as_ptr(), c"x".as_ptr().cast(), 1, 0)
        }),
        "write-memfd" => on_memory_file(|fd| unsafe {
            if libc::write(fd, b"x".as_ptr().cast(), 1) == 1 {
                0
            } else {
                -1
            }
        }),
        // SAFETY (IPC probes): raw calls on scalar arguments and a NUL-terminated name; an
        // object created by mistake is private to the probe or removed with it.
        "shmget" => status(i64::from(unsafe {
            libc::shmget(libc::IPC_PRIVATE, 4096, libc::IPC_CREAT | 0o600)
        })),
        // `IPC_INFO` reports the system's limits into a buffer larger than it fills.
        "shm-info" => status(i64::from(unsafe {
            let mut info: libc::shmid_ds = std::mem::zeroed();
            libc::shmctl(0, libc::IPC_INFO, &mut info)
        })),
        "semget" => status(i64::from(unsafe {
            libc::semget(libc::IPC_PRIVATE, 1, libc::IPC_CREAT | 0o600)
        })),
        "msgget" => status(i64::from(unsafe {
            libc::msgget(libc::IPC_PRIVATE, libc::IPC_CREAT | 0o600)
        })),
        "mq-open" => status(unsafe {
            libc::syscall(
                libc::SYS_mq_open,
                c"restricted-probe".as_ptr(),
                libc::O_RDONLY,
                0,
                std::ptr::null::<libc::mq_attr>(),
            )
        }),
        "mq-unlink" => {
            status(unsafe { libc::syscall(libc::SYS_mq_unlink, c"restricted-probe".as_ptr()) })
        }
        "shared-memory" => fill_memory_files(MEMORY_LIMIT_BYTES + (64 << 20)),
        other => format!("unknown {other}"),
    }
}

/// The filesystem is closed: no file read, directory listing or file creation.
#[test]
fn the_lockdown_denies_the_filesystem() {
    if !lockdown_supported() {
        return;
    }
    for op in ["read-file", "list-directory", "write-file"] {
        let report = probe(op);
        assert!(report.starts_with("denied"), "{op}: {report}");
    }
}

/// No socket of any family can be created, so no connection is possible.
#[test]
fn the_lockdown_denies_the_network() {
    if !lockdown_supported() {
        return;
    }
    for op in ["tcp-connect", "udp-bind", "unix-socket"] {
        assert_eq!(probe(op), format!("denied {}", libc::EPERM), "{op}");
    }
}

/// No program can be run and no process created, traced or moved to a new namespace, while
/// threads still start.
#[test]
fn the_lockdown_denies_processes_but_not_threads() {
    if !lockdown_supported() {
        return;
    }
    for op in ["spawn", "fork", "ptrace", "unshare"] {
        let report = probe(op);
        assert!(report.starts_with("denied"), "{op}: {report}");
    }
    assert_eq!(probe("thread"), "allowed");
}

/// Private writable memory stops at the data limit, and the descriptor limit holds.
#[test]
fn the_lockdown_bounds_memory_and_descriptors() {
    if !lockdown_supported() {
        return;
    }
    assert!(probe("memory").starts_with("denied"));
    assert_eq!(probe("limits"), format!("nofile {DESCRIPTOR_LIMIT}"));
}

/// The helper may query and set its own resource limits but not the client's, which shares
/// its user and whose process id it knows.
#[test]
fn the_lockdown_denies_resource_limits_of_other_processes() {
    if !lockdown_supported() {
        return;
    }
    assert_eq!(probe("prlimit-parent"), format!("denied {}", libc::EPERM));
    assert_eq!(probe("prlimit-self"), "allowed");
    assert_eq!(probe("prlimit-own-pid"), "allowed");
    assert_eq!(probe("limits"), format!("nofile {DESCRIPTOR_LIMIT}"));
}

/// The helper cannot signal its parent, its process group or anyone else, directly, by
/// thread, through a process descriptor or as the owner of a descriptor's I/O signals, nor
/// change its parent's priority, but it can still signal and renice itself.
#[test]
fn the_lockdown_denies_signals_to_other_processes() {
    if !lockdown_supported() {
        return;
    }
    for op in [
        "signal-parent-check",
        "signal-parent",
        "signal-group",
        "tgkill-parent",
        "sigio-parent",
        "pidfd-parent",
        "renice-parent",
    ] {
        assert_eq!(probe(op), format!("denied {}", libc::EPERM), "{op}");
    }
    for op in ["signal-self", "tgkill-self", "renice-self"] {
        assert_eq!(probe(op), "allowed", "{op}");
    }
}

/// No file's mode, owner, times, extended attributes or length can change, by path, which
/// Landlock does not cover, or by descriptor, even on the helper's own memory files, which it
/// can still write.
#[test]
fn the_lockdown_denies_file_attribute_changes() {
    if !lockdown_supported() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), b"probe").unwrap();
    std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o640)).unwrap();
    let path = file.path().to_str().unwrap();
    for op in [
        "chmod",
        "chown",
        "utimes",
        "setxattr",
        "removexattr",
        "truncate",
    ] {
        assert_eq!(
            probe(&format!("{op}={path}")),
            format!("denied {}", libc::EPERM),
            "{op}"
        );
    }
    let metadata = std::fs::metadata(file.path()).unwrap();
    assert_eq!(
        (metadata.len(), metadata.permissions().mode() & 0o777),
        (5, 0o640)
    );
    for op in [
        "ftruncate-memfd",
        "fchmod-memfd",
        "fchown-memfd",
        "fsetxattr-memfd",
    ] {
        assert_eq!(probe(op), format!("denied {}", libc::EPERM), "{op}");
    }
    assert_eq!(probe("write-memfd"), "allowed");
}

/// No System V shared memory, semaphore or message queue, nor any POSIX message queue, can be
/// created or reached, so the helper shares no IPC object with another process of the user.
#[test]
fn the_lockdown_denies_ipc_objects() {
    if !lockdown_supported() {
        return;
    }
    for op in [
        "shmget",
        "shm-info",
        "semget",
        "msgget",
        "mq-open",
        "mq-unlink",
    ] {
        assert_eq!(probe(op), format!("denied {}", libc::EPERM), "{op}");
    }
}

/// Memory files count against the helper's cgroup: filling more than its memory limit with
/// them, which neither `RLIMIT_DATA` nor swap can absorb, gets the helper killed, and the
/// cgroup is removed once the helper has exited.
#[test]
fn the_cgroup_bounds_shared_memory() {
    if !lockdown_supported() {
        return;
    }
    let (report, output, containment) =
        run_probe("shared-memory", |command| contain(command).unwrap());
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(
        (report.as_deref(), output.status.signal()),
        (None, Some(libc::SIGKILL)),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = std::fs::read_to_string(containment.cgroup().join("memory.events")).unwrap();
    assert!(
        events.lines().any(|line| line
            .strip_prefix("oom_kill ")
            .is_some_and(|count| count != "0")),
        "{events}"
    );
    let cgroup = containment.cgroup().to_path_buf();
    drop(containment);
    assert!(!cgroup.exists(), "{}", cgroup.display());
}

/// Launches like [`contain`], descriptors and limits included, but leaves the child in this
/// process's cgroup, as when the helper's cgroup could not be joined.
fn contain_outside_cgroup(command: &mut Command) {
    command.env_clear();
    // SAFETY: the closure makes only raw system calls on fully initialized structs.
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // `CLOSE_RANGE_CLOEXEC`.
            if libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 1u32 << 2) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            for (resource, value) in LIMITS {
                let limit = libc::rlimit {
                    rlim_cur: value as libc::rlim_t,
                    rlim_max: value as libc::rlim_t,
                };
                if libc::setrlimit(resource, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        })
    };
}

/// A helper whose own cgroup does not hold it to the memory limit refuses to lock down, so it
/// never serves.
#[test]
fn a_helper_outside_a_memory_limited_cgroup_fails_its_audit() {
    let report = probe_with("read-file", contain_outside_cgroup);
    assert!(report.starts_with("lockdown-failed"), "{report}");
    assert!(report.contains("memory limit"), "{report}");
}

/// The running helper stays undumpable, so processes of the same user cannot trace it or read
/// its memory, although `exec` resets the flag the parent set before it.
#[test]
fn the_lockdown_leaves_the_helper_undumpable() {
    if !lockdown_supported() {
        return;
    }
    assert_eq!(probe("dumpable"), "dumpable 0");
}

/// A helper launched without containment refuses to lock down, so it never serves: it sees the
/// descriptor this test leaks into it, or the inherited environment.
#[test]
fn an_uncontained_launch_fails_its_audit() {
    if !lockdown_supported() {
        return;
    }
    let file = std::fs::File::open(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    let leaked = file.as_raw_fd();
    // SAFETY: clearing close-on-exec on a descriptor this test owns.
    assert_eq!(unsafe { libc::fcntl(leaked, libc::F_SETFD, 0) }, 0);
    let report = probe_with("read-file", |_| {});
    assert!(report.starts_with("lockdown-failed"), "{report}");
    assert!(
        report.contains("descriptors") || report.contains("environment"),
        "{report}"
    );
    // Contained, the same leak is closed before `exec`.
    assert!(probe("read-file").starts_with("denied"));
    assert!(inherited_descriptors().unwrap().contains(&leaked));
}

/// Launches like [`contain`] but closes no descriptor, as when `close_range` is refused and the
/// fallback closure misses one: the limits still drop below the descriptors left open.
fn contain_without_closing(command: &mut Command) {
    command.env_clear();
    // SAFETY: the closure makes only raw system calls on fully initialized structs.
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            for (resource, value) in LIMITS {
                let limit = libc::rlimit {
                    rlim_cur: value as libc::rlim_t,
                    rlim_max: value as libc::rlim_t,
                };
                if libc::setrlimit(resource, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        })
    };
}

/// A descriptor numbered above the helper's descriptor limit still fails the audit, so a
/// launch whose closure before `exec` missed it never serves.
#[test]
fn a_descriptor_above_the_limit_fails_the_audit() {
    let file = std::fs::File::open(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    let floor = DESCRIPTOR_LIMIT as libc::c_int * 2;
    // SAFETY: duplicating a descriptor this test owns; F_DUPFD leaves close-on-exec clear.
    let high = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD, floor) };
    assert!(high >= floor, "{}", std::io::Error::last_os_error());
    let report = probe_with("read-file", contain_without_closing);
    // SAFETY: closing the duplicate this test made.
    unsafe { libc::close(high) };
    assert!(report.starts_with("lockdown-failed"), "{report}");
    assert!(
        report.contains("descriptors") && report.contains(&high.to_string()),
        "{report}"
    );
}
