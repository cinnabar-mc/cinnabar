//! Builds an offered release's carriers while the current client runs, so that release's first
//! launch only promotes them. At most one set waits under the prepare workspace: `building` while
//! a run is in progress or was interrupted, `ready` once complete. Launch-time setup resumes
//! from either when it cannot promote.

use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result, ensure};
use assets::carriers::{self, STAMP_FILE};
use launcher::install_layout::InstallLayout;

use super::{
    download, fs_retry, prepare,
    runner::{self, Cancelled, Compiler},
};

const NEXT_DIR: &str = "next";
const READY: &str = "ready";
const BUILDING: &str = "building";
/// Holds a seed until it is complete, so `building` never starts from a partial copy.
const SEEDING: &str = "seeding";
/// The offered kit, linked so pruning or installing its candidate cannot change it mid-run.
const KIT: &str = "kit";
const LOG_FILE: &str = "first-run-next.log";
/// Most carriers a background run builds at once, whatever the core count.
const MAX_JOBS: usize = 2;

/// The latest request and the thread serving it.
struct Job {
    kit: Option<PathBuf>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);

/// Builds the carriers of the release `kit` belongs to on a background thread, replacing any
/// earlier request; `None` cancels that work and discards its output. Repeats are ignored.
pub fn prepare_next_release(layout: &InstallLayout, kit: Option<&Path>) {
    if !layout.is_installed() {
        return;
    }
    let mut job = JOB.lock().unwrap_or_else(PoisonError::into_inner);
    if job.as_ref().is_some_and(|job| job.kit.as_deref() == kit) {
        return;
    }
    let previous = job.take();
    if let Some(previous) = &previous {
        previous.cancel.store(true, Ordering::Relaxed);
    }
    let kit = kit.map(Path::to_path_buf);
    let cancel = Arc::new(AtomicBool::new(false));
    let spawned = {
        let (layout, kit, cancel) = (layout.clone(), kit.clone(), Arc::clone(&cancel));
        thread::Builder::new()
            .name("next-release-assets".into())
            .spawn(move || {
                // One run at a time owns the staged set.
                if let Some(thread) = previous.and_then(|previous| previous.thread) {
                    let _ = thread.join();
                }
                let _span = tracing::info_span!("next_release_assets").entered();
                let result = match &kit {
                    Some(kit) => build(&layout, kit, &cancel),
                    None => {
                        discard(&layout);
                        Ok(())
                    }
                };
                if let Err(error) = result
                    && !error.is::<Cancelled>()
                {
                    tracing::info!("background asset preparation stopped: {error:#}");
                }
            })
    };
    match spawned {
        Ok(thread) => {
            *job = Some(Job {
                kit,
                cancel,
                thread: Some(thread),
            });
        }
        Err(error) => tracing::warn!("could not start background asset preparation: {error}"),
    }
}

/// Holds the staged set and the run's kit copy, beside the cached pack the run reads.
fn next_dir(layout: &InstallLayout) -> PathBuf {
    layout.prepare_workspace().join(NEXT_DIR)
}

/// Removes every staged set and kit.
fn discard(layout: &InstallLayout) {
    let next = next_dir(layout);
    if next.exists() {
        let _ = fs_retry::remove_dir_all(&next);
    }
}

/// Builds `source`'s carriers into `building` from a private copy of the kit, renaming the set to
/// `ready` once complete. A new pack pin is left to launch-time setup, which shows its download.
pub(super) fn build(layout: &InstallLayout, source: &Path, cancel: &AtomicBool) -> Result<()> {
    let next = next_dir(layout);
    let kit = next.join(KIT);
    if kit.exists() {
        fs::remove_dir_all(&kit)?;
    }
    link_tree(source, &kit).with_context(|| format!("copy {}", source.display()))?;
    let result = build_from(layout, &next, &kit, cancel);
    let _ = fs::remove_dir_all(&kit);
    result
}

/// Resumes from `ready` or `building` when present, else seeds from the published set.
fn build_from(layout: &InstallLayout, next: &Path, kit: &Path, cancel: &AtomicBool) -> Result<()> {
    let workspace = layout.prepare_workspace();
    let compiler = Compiler {
        kit: kit.to_owned(),
        workspace: workspace.clone(),
        log: None,
        cancel,
        #[cfg(unix)]
        own_group: true,
        background: true,
    };
    let (ready, building) = (next.join(READY), next.join(BUILDING));
    if ready.is_dir() {
        if compiler.check(&ready)?.is_current() {
            return Ok(());
        }
        // A set finished for an earlier offer is this run's starting point.
        replace(&ready, &building)?;
    }
    if !building.is_dir() {
        let prepared = layout.prepared_assets_dir();
        let selection = compiler.check(&prepared)?;
        if selection.is_current() {
            return Ok(());
        }
        seed(&prepared, &next.join(SEEDING), &building, &selection.stale)?;
    }
    let selection = compiler.check(&building)?;
    if selection.needs_pack && !download::pack_cached(kit, &workspace)? {
        return Ok(());
    }
    if !selection.is_current() {
        fs::create_dir_all(layout.log_dir())?;
        let log = File::create(layout.log_dir().join(LOG_FILE))?;
        compiler.prepare_in_background(&building, jobs(), log)?;
    }
    ensure!(
        carriers::required_present(&building),
        "background preparation left required carriers missing"
    );
    replace(&building, &ready)
}

/// Background runs leave most cores to the game.
fn jobs() -> usize {
    thread::available_parallelism()
        .map_or(1, |cores| cores.get() / 4)
        .clamp(1, MAX_JOBS)
}

/// Copies `prepared` without the outputs of `stale` carriers into `seeding`, its stamp last, and
/// only then renames the copy to `building`.
fn seed(prepared: &Path, seeding: &Path, building: &Path, stale: &[String]) -> Result<()> {
    if seeding.exists() {
        fs::remove_dir_all(seeding)?;
    }
    fs::create_dir_all(seeding)?;
    let rebuilt: Vec<PathBuf> = stale
        .iter()
        .filter_map(|name| carriers::by_name(name))
        .flat_map(|carrier| carrier.outputs(prepared))
        .collect();
    if prepared.is_dir() {
        for entry in fs::read_dir(prepared)? {
            let entry = entry?;
            let (from, to) = (entry.path(), seeding.join(entry.file_name()));
            if rebuilt.contains(&from) || entry.file_name() == STAMP_FILE {
                continue;
            }
            if entry.file_type()?.is_dir() {
                runner::copy_tree(&from, &to)?;
            } else {
                fs::copy(&from, &to)?;
            }
        }
        let stamp = prepared.join(STAMP_FILE);
        if stamp.is_file() {
            fs::copy(&stamp, seeding.join(STAMP_FILE))?;
        }
    }
    fs::rename(seeding, building).context("publish the seeded carriers")
}

/// Mirrors `from` at `to` with hard links, copying where the filesystem cannot link.
fn link_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            link_tree(&entry.path(), &target)?;
        } else if fs::hard_link(entry.path(), &target).is_err() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Moves `from` to `to`, replacing what `to` held.
fn replace(from: &Path, to: &Path) -> Result<()> {
    if to.exists() {
        fs_retry::remove_dir_all(to)?;
    }
    fs_retry::rename(from, to).with_context(|| format!("move {}", from.display()))
}

/// Publishes a complete background set when the installed kit finds it current, so this launch
/// skips setup. Anything else stays for setup to resume from.
pub(super) fn promote(layout: &InstallLayout) -> bool {
    if !layout.is_installed() {
        return false;
    }
    let next = next_dir(layout);
    let ready = next.join(READY);
    if !ready.is_dir()
        || !carriers::required_present(&ready)
        || !prepare::check(layout, &ready).is_ok_and(|selection| selection.is_current())
    {
        return false;
    }
    let prepared = layout.prepared_assets_dir();
    runner::recover(&prepared);
    if let Err(error) = runner::publish(&ready, &prepared) {
        tracing::warn!("could not promote background-prepared carriers: {error:#}");
        return false;
    }
    discard(layout);
    true
}

/// Moves the background set, finished or not, to `staged` for setup to resume from, then clears
/// the staging; false when no set could be moved.
pub(super) fn take(layout: &InstallLayout, staged: &Path) -> Result<bool> {
    let next = next_dir(layout);
    if !next.exists() {
        return Ok(false);
    }
    if let Some(parent) = staged.parent() {
        fs::create_dir_all(parent)?;
    }
    let taken = [READY, BUILDING].iter().any(|name| {
        let set = next.join(name);
        set.is_dir() && fs_retry::rename(&set, staged).is_ok()
    });
    discard(layout);
    Ok(taken)
}

#[cfg(all(test, unix))]
mod tests;
