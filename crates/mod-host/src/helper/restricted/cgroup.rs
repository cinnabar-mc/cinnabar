//! The cgroup v2 limits of a restricted helper. The client creates one cgroup per helper below
//! the nearest ancestor of its own cgroup that it may manage and that enables the memory and pids
//! controllers for its children, as a systemd user session delegates, and the helper joins it
//! between `fork` and `exec`. The memory limit covers every page the helper is charged for,
//! shared memory files included, which `RLIMIT_DATA` does not count; swap is closed so the limit
//! cannot spill there. The task limit bounds its threads, which `RLIMIT_NPROC` would count
//! against every process of the user. Where the ancestor also delegates the cpu controller, the
//! helper gets one core; otherwise its CPU time is unbounded. Without memory and pids the
//! restricted launch fails closed.

use super::{MEMORY_LIMIT_BYTES, TASK_LIMIT};
use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io::Write,
    os::{fd::AsRawFd, unix::ffi::OsStrExt},
    path::{Path, PathBuf},
};

/// Where the cgroup v2 hierarchy is mounted.
const ROOT: &str = "/sys/fs/cgroup";
/// This process's cgroup membership.
const MEMBERSHIP: &str = "/proc/self/cgroup";
/// The name prefix of the cgroups made for helpers; a random suffix follows.
const PREFIX: &str = "restricted-helper-";
/// The controllers a helper's cgroup must have.
const REQUIRED: [&str; 2] = ["memory", "pids"];
/// The helper's CPU bandwidth, `cpu.max`: one core, a full period of every period.
const CPU_MAX: &str = "100000 100000";

/// A helper's cgroup, with its limits set. The client holds an exclusive lock on its directory
/// while the cgroup is in use, so a later launch can tell a cgroup whose client is gone. Dropping
/// it removes the cgroup, which the kernel allows once the helper has exited, so it must outlive
/// the helper's process.
#[derive(Debug)]
pub struct Cgroup {
    path: PathBuf,
    /// The cgroup's `cgroup.procs`, open for writing, through which the child joins it.
    procs: File,
    /// The cgroup's directory, locked for as long as the client uses it.
    _lock: File,
}

impl Cgroup {
    /// Creates a cgroup for one helper below the nearest ancestor of this process's cgroup that
    /// this user may manage and whose children carry the memory and pids controllers.
    pub fn create() -> Result<Self> {
        let membership = std::fs::read_to_string(MEMBERSHIP)
            .with_context(|| format!("cannot read {MEMBERSHIP}"))?;
        Self::create_in(Path::new(ROOT), &membership)
    }

    /// [`Self::create`] against the hierarchy mounted at `root`, for a process whose
    /// `/proc/self/cgroup` reads `membership`.
    fn create_in(root: &Path, membership: &str) -> Result<Self> {
        let parent = delegated_ancestor(root, own_path(membership)?)?;
        remove_stale(&parent);
        let path = parent.join(format!("{PREFIX}{}", random_suffix()?));
        std::fs::create_dir(&path)
            .with_context(|| format!("cannot create the helper cgroup {}", path.display()))?;
        // Removes the cgroup again if it cannot be completed.
        let created = Removal(path);
        let path = &created.0;
        let lock = lock(path, true)?
            .with_context(|| format!("{} is locked by another client", path.display()))?;
        // The memory limit is set only once the lock is held, so an unlocked cgroup with a
        // limit belongs to a client that is gone; see [`remove_stale`].
        write_existing(&path.join("memory.max"), &MEMORY_LIMIT_BYTES.to_string())?;
        write_existing(&path.join("memory.swap.max"), "0")?;
        write_existing(&path.join("pids.max"), &TASK_LIMIT.to_string())?;
        if path.join("cpu.max").exists() {
            write_existing(&path.join("cpu.max"), CPU_MAX)?;
        }
        let procs = OpenOptions::new()
            .write(true)
            .open(path.join("cgroup.procs"))
            .with_context(|| format!("cannot open {}/cgroup.procs", path.display()))?;
        let path = created.keep();
        Ok(Self {
            path,
            procs,
            _lock: lock,
        })
    }

    /// The cgroup's directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A new descriptor of the cgroup's `cgroup.procs`; writing `0` to it moves the writer in.
    /// It is close-on-exec, so the helper does not inherit it.
    pub(super) fn procs(&self) -> std::io::Result<File> {
        self.procs.try_clone()
    }
}

#[cfg(test)]
impl Cgroup {
    /// Releases the cgroup's lock and descriptors but leaves the cgroup, as a client that dies
    /// does, and returns its directory.
    fn abandon(self) -> PathBuf {
        let this = std::mem::ManuallyDrop::new(self);
        // SAFETY: each field is moved out once and `this` is never dropped or used again.
        let (path, procs, lock) = unsafe {
            (
                std::ptr::read(&this.path),
                std::ptr::read(&this.procs),
                std::ptr::read(&this._lock),
            )
        };
        drop((procs, lock));
        path
    }
}

impl Drop for Cgroup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.path);
    }
}

/// Removes a cgroup directory when dropped unless kept.
struct Removal(PathBuf);

impl Removal {
    /// Keeps the directory and returns its path.
    fn keep(self) -> PathBuf {
        let path = self.0.clone();
        std::mem::forget(self);
        path
    }
}

impl Drop for Removal {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

/// Sixteen random hexadecimal digits from the kernel's generator.
fn random_suffix() -> Result<String> {
    let mut bytes = [0u8; 8];
    // SAFETY: getrandom fills at most `bytes.len()` bytes of the buffer.
    let filled = unsafe { libc::getrandom(bytes.as_mut_ptr().cast(), bytes.len(), 0) };
    ensure!(
        filled == bytes.len() as isize,
        "cannot name the helper cgroup: {}",
        std::io::Error::last_os_error()
    );
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Takes an exclusive lock on the directory `path`, waiting for it if `wait`: the locked
/// directory, or none when another open description holds the lock and `wait` is false. The
/// lock lasts until every descriptor of the returned file is closed, as when its process exits.
fn lock(path: &Path, wait: bool) -> Result<Option<File>> {
    let directory = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let operation = if wait {
        libc::LOCK_EX
    } else {
        libc::LOCK_EX | libc::LOCK_NB
    };
    // SAFETY: flock on a descriptor this function owns.
    if unsafe { libc::flock(directory.as_raw_fd(), operation) } == 0 {
        return Ok(Some(directory));
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
        return Ok(None);
    }
    Err(error).with_context(|| format!("cannot lock {}", path.display()))
}

/// The limits the calling process's own cgroup holds it to, with `max` as `u64::MAX`. Fails
/// when one cannot be read, so an audit built on it fails closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// `memory.max`.
    pub memory: u64,
    /// `memory.swap.max`.
    pub swap: u64,
    /// `pids.max`.
    pub tasks: u64,
}

impl Limits {
    /// Reads the calling process's own cgroup's limits.
    pub fn own() -> Result<Self> {
        let membership = std::fs::read_to_string(MEMBERSHIP)
            .with_context(|| format!("cannot read {MEMBERSHIP}"))?;
        let cgroup = Path::new(ROOT).join(own_path(&membership)?);
        let read = |name: &str| -> Result<u64> {
            let path = cgroup.join(name);
            let value = std::fs::read_to_string(&path)
                .with_context(|| format!("cannot read {}", path.display()))?;
            match value.trim() {
                "max" => Ok(u64::MAX),
                value => value
                    .parse()
                    .with_context(|| format!("{} reads {value:?}", path.display())),
            }
        };
        Ok(Self {
            memory: read("memory.max")?,
            swap: read("memory.swap.max")?,
            tasks: read("pids.max")?,
        })
    }

    /// Whether these limits are at least as tight as a helper's.
    pub fn hold_a_helper(self) -> bool {
        self.memory <= MEMORY_LIMIT_BYTES && self.swap == 0 && self.tasks <= TASK_LIMIT
    }
}

/// The cgroup v2 path, relative to the hierarchy's root, that `membership` names.
fn own_path(membership: &str) -> Result<&str> {
    membership
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .map(|path| path.trim_start_matches('/'))
        .context("the process is in no cgroup v2 hierarchy")
}

/// The nearest of `own` and its ancestors below `root` that enables the memory and pids
/// controllers for its children and in which this user may create cgroups and move processes.
fn delegated_ancestor(root: &Path, own: &str) -> Result<PathBuf> {
    let mut candidate = root.join(own);
    while candidate.starts_with(root) && candidate != root {
        let controllers =
            std::fs::read_to_string(candidate.join("cgroup.subtree_control")).unwrap_or_default();
        let enabled = REQUIRED
            .iter()
            .all(|required| controllers.split_whitespace().any(|name| name == *required));
        if enabled
            && accessible(&candidate, libc::W_OK | libc::X_OK)
            && accessible(&candidate.join("cgroup.procs"), libc::W_OK)
        {
            return Ok(candidate);
        }
        candidate.pop();
    }
    bail!(
        "no cgroup that delegates the memory and pids controllers to this user holds the client \
         (it runs in /{own}); a restricted helper needs one to limit its memory and tasks"
    )
}

/// Whether this user has `mode` access to `path`.
fn accessible(path: &Path, mode: libc::c_int) -> bool {
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `path` is NUL-terminated and outlives the call.
    unsafe { libc::access(path.as_ptr(), mode) == 0 }
}

/// Writes `value` to the existing control file `path`; a missing file, as when the controller
/// is not enabled, is an error.
fn write_existing(path: &Path, value: &str) -> Result<()> {
    OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|mut file| file.write_all(value.as_bytes()))
        .with_context(|| format!("cannot set {} to {value}", path.display()))
}

/// Clears the helper cgroups in `parent` whose clients are gone: those no client holds locked
/// and whose memory limit is set, which their client did only while holding the lock. Helpers
/// still in one, which outlived their client, are killed through `cgroup.kill`, and the cgroup
/// is removed once empty, by this launch or a later one. A cgroup another client is still
/// creating has no limit yet and stays.
fn remove_stale(parent: &Path) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_name().as_bytes().starts_with(PREFIX.as_bytes()) {
            continue;
        }
        let path = entry.path();
        let Ok(Some(_lock)) = lock(&path, false) else {
            continue;
        };
        let limited = std::fs::read_to_string(path.join("memory.max"))
            .is_ok_and(|limit| limit.trim() != "max");
        if !limited {
            continue;
        }
        let populated = std::fs::read_to_string(path.join("cgroup.events"))
            .is_ok_and(|events| events.lines().any(|line| line == "populated 1"));
        if populated {
            let _ = write_existing(&path.join("cgroup.kill"), "1");
        }
        let _ = std::fs::remove_dir(&path);
    }
}

/// Fails unless the calling process's own cgroup holds it to the helper's memory and task
/// limits with no swap.
pub(super) fn audit() -> Result<()> {
    let limits = Limits::own().context("the helper cannot read its cgroup's limits")?;
    ensure!(
        limits.hold_a_helper(),
        "the helper runs without its memory and task limits ({limits:?})"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hierarchy at `root` with the cgroups `path` names, each enabling `controllers` for its
    /// children.
    fn hierarchy(root: &Path, path: &str, controllers: &str) {
        let mut cgroup = root.to_path_buf();
        for part in path.split('/') {
            cgroup.push(part);
            std::fs::create_dir_all(&cgroup).unwrap();
            std::fs::write(cgroup.join("cgroup.subtree_control"), controllers).unwrap();
            std::fs::write(cgroup.join("cgroup.procs"), "").unwrap();
        }
    }

    #[test]
    fn membership_names_the_unified_hierarchy() {
        let membership = "1:name=systemd:/legacy\n0::/user.slice/app.slice/run.scope\n";
        assert_eq!(
            own_path(membership).unwrap(),
            "user.slice/app.slice/run.scope"
        );
        assert!(own_path("1:memory:/legacy\n").is_err());
    }

    /// Without an ancestor that delegates both the memory and the pids controller, no cgroup
    /// is made and the launch fails with the reason.
    #[test]
    fn missing_memory_or_pids_delegation_fails_closed() {
        for controllers in ["cpu io pids", "cpu io memory"] {
            let root = tempfile::tempdir().unwrap();
            hierarchy(root.path(), "user.slice/app.slice/run.scope", controllers);
            let error = Cgroup::create_in(root.path(), "0::/user.slice/app.slice/run.scope\n")
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("delegates the memory and pids controllers"),
                "{error}"
            );
            assert!(error.contains("user.slice/app.slice/run.scope"), "{error}");
        }
    }

    /// An ancestor that claims the controllers but whose new child has no memory limit to set,
    /// as on a hierarchy that is not cgroup v2, fails closed and leaves nothing behind.
    #[test]
    fn a_cgroup_without_a_memory_limit_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        hierarchy(root.path(), "user.slice/app.slice/run.scope", "");
        hierarchy(root.path(), "user.slice/app.slice", "memory pids");
        let error =
            Cgroup::create_in(root.path(), "0::/user.slice/app.slice/run.scope\n").unwrap_err();
        assert!(format!("{error:#}").contains("memory.max"), "{error:#}");
        let parent = root.path().join("user.slice/app.slice");
        let children: Vec<_> = std::fs::read_dir(&parent)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(PREFIX))
            .collect();
        assert!(children.is_empty(), "{children:?}");
    }

    /// Helper cgroups get distinct random names.
    #[test]
    fn helper_cgroup_names_are_random() {
        let (first, second) = (random_suffix().unwrap(), random_suffix().unwrap());
        assert_eq!(first.len(), 16);
        assert_ne!(first, second);
    }

    /// Set for the process that [`a_gone_clients_helpers_are_killed_and_their_cgroup_removed`]
    /// leaves in a cgroup.
    const RESIDENT: &str = "RESTRICTED_CGROUP_RESIDENT";

    /// Does nothing in an ordinary run. As the resident, waits until its stdin closes.
    #[test]
    fn resident_waiting_for_stdin() {
        if std::env::var_os(RESIDENT).is_some() {
            let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        }
    }

    /// A cgroup whose client released its lock without removing it, as when the client died, is
    /// cleared by the next launch: the helper that outlived it is killed and the cgroup removed.
    /// A cgroup whose client still holds it stays.
    #[test]
    fn a_gone_clients_helpers_are_killed_and_their_cgroup_removed() {
        use std::os::unix::process::{CommandExt, ExitStatusExt};
        let (abandoned, kept) = match (Cgroup::create(), Cgroup::create()) {
            (Ok(abandoned), Ok(kept)) => (abandoned, kept),
            (Err(error), _) | (_, Err(error)) => {
                eprintln!("skipping: missing a delegated cgroup v2 hierarchy ({error:#})");
                return;
            }
        };
        let procs = abandoned.procs().unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "helper::restricted::cgroup::tests::resident_waiting_for_stdin",
            ])
            .env(RESIDENT, "1")
            .stdin(std::process::Stdio::piped());
        // SAFETY: the closure makes one raw write on a descriptor it owns.
        unsafe {
            command.pre_exec(move || {
                if libc::write(procs.as_raw_fd(), b"0".as_ptr().cast(), 1) == 1 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            })
        };
        let mut resident = command.spawn().unwrap();
        // The client is gone: its lock is released and its cgroup left behind.
        let path = abandoned.abandon();
        let parent = path.parent().unwrap().to_path_buf();
        remove_stale(&parent);
        let status = resident.wait().unwrap();
        assert_eq!(status.signal(), Some(libc::SIGKILL), "{status}");
        remove_stale(&parent);
        assert!(!path.exists(), "{}", path.display());
        assert!(kept.path().exists());
    }
}
