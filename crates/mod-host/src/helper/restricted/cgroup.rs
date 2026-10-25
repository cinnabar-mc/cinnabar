//! The cgroup v2 memory limit of a restricted helper. The client creates one cgroup per helper
//! below the nearest ancestor of its own cgroup that it may manage and that enables the memory
//! controller for its children, as a systemd user session delegates, and the helper joins it
//! between `fork` and `exec`. The limit covers every page the helper is charged for, shared
//! memory files included, which `RLIMIT_DATA` does not count; swap is closed so the limit cannot
//! spill there. Without such an ancestor the restricted launch fails closed.

use super::MEMORY_LIMIT_BYTES;
use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io::Write,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Where the cgroup v2 hierarchy is mounted.
const ROOT: &str = "/sys/fs/cgroup";
/// This process's cgroup membership.
const MEMBERSHIP: &str = "/proc/self/cgroup";
/// The name prefix of the cgroups made for helpers; the client's process id and a sequence
/// number follow.
const PREFIX: &str = "restricted-helper-";

/// A helper's cgroup, with its memory limit set. Dropping it removes the cgroup, which the
/// kernel allows once the helper has exited, so it must outlive the helper's process.
#[derive(Debug)]
pub struct Cgroup {
    path: PathBuf,
    /// The cgroup's `cgroup.procs`, open for writing, through which the child joins it.
    procs: File,
}

impl Cgroup {
    /// Creates a cgroup for one helper below the nearest ancestor of this process's cgroup that
    /// this user may manage and whose children carry the memory controller.
    pub fn create() -> Result<Self> {
        let membership = std::fs::read_to_string(MEMBERSHIP)
            .with_context(|| format!("cannot read {MEMBERSHIP}"))?;
        Self::create_in(Path::new(ROOT), &membership)
    }

    /// [`Self::create`] against the hierarchy mounted at `root`, for a process whose
    /// `/proc/self/cgroup` reads `membership`.
    fn create_in(root: &Path, membership: &str) -> Result<Self> {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let parent = delegated_ancestor(root, own_path(membership)?)?;
        remove_stale(&parent);
        let client = std::process::id();
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!("{PREFIX}{client}-{sequence}"));
        std::fs::create_dir(&path)
            .with_context(|| format!("cannot create the helper cgroup {}", path.display()))?;
        // Removes the cgroup again if it cannot be completed.
        let created = Removal(path);
        let path = &created.0;
        write_existing(&path.join("memory.max"), &MEMORY_LIMIT_BYTES.to_string())?;
        write_existing(&path.join("memory.swap.max"), "0")?;
        let procs = OpenOptions::new()
            .write(true)
            .open(path.join("cgroup.procs"))
            .with_context(|| format!("cannot open {}/cgroup.procs", path.display()))?;
        let path = created.keep();
        Ok(Self { path, procs })
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

/// The memory limits the calling process runs under: its own cgroup's `memory.max` and
/// `memory.swap.max`, with `max` as `u64::MAX`. Fails when either cannot be read, so an audit
/// built on it fails closed.
pub fn own_limits() -> Result<(u64, u64)> {
    let membership =
        std::fs::read_to_string(MEMBERSHIP).with_context(|| format!("cannot read {MEMBERSHIP}"))?;
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
    Ok((read("memory.max")?, read("memory.swap.max")?))
}

/// The cgroup v2 path, relative to the hierarchy's root, that `membership` names.
fn own_path(membership: &str) -> Result<&str> {
    membership
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .map(|path| path.trim_start_matches('/'))
        .context("the process is in no cgroup v2 hierarchy")
}

/// The nearest of `own` and its ancestors below `root` that enables the memory controller for
/// its children and in which this user may create cgroups and move processes.
fn delegated_ancestor(root: &Path, own: &str) -> Result<PathBuf> {
    let mut candidate = root.join(own);
    while candidate.starts_with(root) && candidate != root {
        let controllers = std::fs::read_to_string(candidate.join("cgroup.subtree_control"));
        let memory = controllers
            .as_deref()
            .is_ok_and(|controllers| controllers.split_whitespace().any(|name| name == "memory"));
        if memory
            && accessible(&candidate, libc::W_OK | libc::X_OK)
            && accessible(&candidate.join("cgroup.procs"), libc::W_OK)
        {
            return Ok(candidate);
        }
        candidate.pop();
    }
    bail!(
        "no cgroup that delegates the memory controller to this user holds the client (it runs \
         in /{own}); a restricted helper needs one to limit its memory"
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

/// Removes the empty helper cgroups that clients no longer running left in `parent`. A cgroup
/// that still holds a process cannot be removed and stays.
fn remove_stale(parent: &Path) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let client = name
            .to_str()
            .and_then(|name| name.strip_prefix(PREFIX))
            .and_then(|rest| rest.split_once('-'))
            .and_then(|(client, _)| client.parse::<libc::pid_t>().ok());
        let Some(client) = client else {
            continue;
        };
        // SAFETY: signal 0 only checks that the process exists.
        let gone = unsafe { libc::kill(client, 0) } != 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        if gone {
            let _ = std::fs::remove_dir(entry.path());
        }
    }
}

/// Fails unless the calling process's own cgroup holds it to the helper's memory limit with no
/// swap.
pub(super) fn audit() -> Result<()> {
    let (memory, swap) = own_limits().context("the helper cannot read its memory limit")?;
    ensure!(
        memory <= MEMORY_LIMIT_BYTES && swap == 0,
        "the helper runs without its memory limit (memory.max {memory}, memory.swap.max {swap})"
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

    /// Without an ancestor that delegates the memory controller, no cgroup is made and the
    /// launch fails with the reason.
    #[test]
    fn no_delegated_memory_controller_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        hierarchy(root.path(), "user.slice/app.slice/run.scope", "cpu io pids");
        let error = Cgroup::create_in(root.path(), "0::/user.slice/app.slice/run.scope\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("delegates the memory controller"), "{error}");
        assert!(error.contains("user.slice/app.slice/run.scope"), "{error}");
    }

    /// An ancestor that claims the memory controller but whose new child has no memory limit
    /// to set, as on a hierarchy that is not cgroup v2, fails closed and leaves nothing behind.
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

    /// Empty cgroups left by clients that no longer run are removed; the current client's stay.
    #[test]
    fn stale_helper_cgroups_are_removed() {
        let parent = tempfile::tempdir().unwrap();
        let stale = parent
            .path()
            .join(format!("{PREFIX}{}-0", libc::pid_t::MAX));
        let current = parent
            .path()
            .join(format!("{PREFIX}{}-0", std::process::id()));
        let foreign = parent.path().join("app-other.scope");
        for path in [&stale, &current, &foreign] {
            std::fs::create_dir(path).unwrap();
        }
        remove_stale(parent.path());
        assert!(!stale.exists());
        assert!(current.exists() && foreign.exists());
    }
}
