//! Fault injection against the restricted helper's lockdown on Linux. Each probe re-runs this
//! test binary as a child launched exactly as the restricted helper is (contained before
//! `exec`), which locks itself down and then attempts one forbidden operation and reports
//! whether the kernel refused it.

#![cfg(target_os = "linux")]

use mod_host::helper::restricted::{
    DATA_LIMIT_BYTES, DESCRIPTOR_LIMIT, LIMITS, contain, inherited_descriptors, lock_down,
};
use std::{
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::Command,
};

/// The probe child's argument prefix: libtest takes it as one more name filter.
const PROBE: &str = "probe-op:";

/// Runs `op` in a contained child that locks itself down first, and returns its report.
fn probe(op: &str) -> String {
    probe_with(op, contain)
}

/// Runs `op` in a child that `launch` prepares and that locks itself down first, and returns
/// its report.
fn probe_with(op: &str, launch: fn(&mut Command)) -> String {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "probe_child",
        "--nocapture",
        "--test-threads=1",
        &format!("{PROBE}{op}"),
    ]);
    launch(&mut command);
    let output = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find_map(|line| line.split_once("PROBE ").map(|(_, report)| report))
        .map(str::to_owned)
        .unwrap_or_else(|| {
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

/// Attempts one operation the lockdown must refuse, or one it must leave.
fn run(op: &str) -> String {
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
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
        "dumpable" => format!("dumpable {}", unsafe {
            libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0)
        }),
        other => format!("unknown {other}"),
    }
}

/// The filesystem is closed: no file read, directory listing or file creation.
#[test]
fn the_lockdown_denies_the_filesystem() {
    for op in ["read-file", "list-directory", "write-file"] {
        let report = probe(op);
        assert!(report.starts_with("denied"), "{op}: {report}");
    }
}

/// No socket of any family can be created, so no connection is possible.
#[test]
fn the_lockdown_denies_the_network() {
    for op in ["tcp-connect", "udp-bind", "unix-socket"] {
        assert_eq!(probe(op), format!("denied {}", libc::EPERM), "{op}");
    }
}

/// No program can be run and no process created, traced or moved to a new namespace, while
/// threads still start.
#[test]
fn the_lockdown_denies_processes_but_not_threads() {
    for op in ["spawn", "fork", "ptrace", "unshare"] {
        let report = probe(op);
        assert!(report.starts_with("denied"), "{op}: {report}");
    }
    assert_eq!(probe("thread"), "allowed");
}

/// Private writable memory stops at the data limit, and the descriptor limit holds.
#[test]
fn the_lockdown_bounds_memory_and_descriptors() {
    assert!(probe("memory").starts_with("denied"));
    assert_eq!(probe("limits"), format!("nofile {DESCRIPTOR_LIMIT}"));
}

/// The running helper stays undumpable, so processes of the same user cannot trace it or read
/// its memory, although `exec` resets the flag the parent set before it.
#[test]
fn the_lockdown_leaves_the_helper_undumpable() {
    assert_eq!(probe("dumpable"), "dumpable 0");
}

/// A helper launched without containment refuses to lock down, so it never serves: it sees the
/// descriptor this test leaks into it, or the inherited environment.
#[test]
fn an_uncontained_launch_fails_its_audit() {
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
