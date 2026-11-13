//! The stage a run's renderer last reached, kept on disk so the next launch knows where the
//! run ended.
//!
//! The marker is rewritten only when the stage or the run's verdict changes, never per frame.
//! Each write replaces the file by rename, so a process that dies mid-write leaves the
//! previous marker and never a partial one. Writes are not flushed to storage: the marker has
//! to outlive this process, not a power cut.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

use render_api::SafeGraphics;
use serde::{Deserialize, Serialize};

use super::owner::Owner;

/// The marker's file name inside the renderer's data root.
pub(super) const FILE: &str = "renderer-run.json";
/// A marker is a few short fields; anything larger is not one.
const MAX_BYTES: u64 = 4096;

/// What the renderer was doing, in the order a session passes through.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStage {
    /// Creating the device and presenting the first frame.
    Startup,
    /// The menu is up and no world pipelines are compiling.
    Menu,
    /// World pipelines compile behind the menu.
    WarmUp,
    /// Joining a world, up to its first terrain.
    Joining,
    InWorld,
}

impl RunStage {
    /// The name the marker saves, for the startup log.
    pub fn name(self) -> String {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::String(name)) => name,
            other => unreachable!("stages serialize as names, not {other:?}"),
        }
    }
}

/// How a run ended, when the process could still say. Later variants are stronger evidence
/// about the renderer and replace earlier ones, never the reverse.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEnd {
    /// An orderly exit or a returned startup error.
    Clean,
    /// A panic outside graphics code.
    Panic,
    /// A panic raised in graphics code.
    RendererPanic,
    /// The renderer stopped on a GPU error, including a lost device.
    GpuFailure,
}

/// One run's record. A run killed without notice leaves `end` empty.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunMarker {
    pub stage: RunStage,
    #[serde(default)]
    pub end: Option<RunEnd>,
    /// The [`SafeGraphics`] level the run used.
    #[serde(default)]
    pub level: u8,
    /// The run met the bar for a session the renderer survived.
    #[serde(default)]
    pub healthy: bool,
    /// When the run began, in milliseconds since the Unix epoch.
    #[serde(default)]
    pub started_ms: u64,
    /// The process that wrote the marker, so a second run in one process is not read as a
    /// death of the first.
    #[serde(default)]
    pub(super) owner: Option<Owner>,
}

/// What the previous run left behind.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Previous {
    /// No earlier run wrote a marker here.
    Missing,
    /// A file exists but is not a marker; it is evidence of nothing.
    Unreadable,
    Marker(RunMarker),
}

/// Reads the previous run's marker without changing it.
pub(super) fn previous(root: &Path) -> Previous {
    let path = root.join(FILE);
    match fs::metadata(&path) {
        Err(_) => return Previous::Missing,
        Ok(metadata) if metadata.len() > MAX_BYTES => return Previous::Unreadable,
        Ok(_) => {}
    }
    fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .map_or(Previous::Unreadable, Previous::Marker)
}

/// One run's marker and where it is kept.
pub(super) struct Breadcrumb {
    path: PathBuf,
    marker: RunMarker,
    /// A failed write is logged once, not at every later transition.
    write_failed: bool,
}

impl Breadcrumb {
    /// Starts a run at [`RunStage::Startup`], replacing the previous run's marker.
    pub(super) fn begin(
        root: &Path,
        level: SafeGraphics,
        started_ms: u64,
        owner: Option<Owner>,
    ) -> Self {
        let mut breadcrumb = Self {
            path: root.join(FILE),
            marker: RunMarker {
                stage: RunStage::Startup,
                end: None,
                level: level.level(),
                healthy: false,
                started_ms,
                owner,
            },
            write_failed: false,
        };
        breadcrumb.write();
        breadcrumb
    }

    /// Records a stage change; the same stage again writes nothing.
    pub(super) fn stage(&mut self, stage: RunStage) {
        if self.marker.stage != stage {
            self.marker.stage = stage;
            self.write();
        }
    }

    /// Records that the renderer survived this session.
    pub(super) fn healthy(&mut self) {
        if !self.marker.healthy {
            self.marker.healthy = true;
            self.write();
        }
    }

    /// Records how the run ends, keeping the strongest evidence.
    pub(super) fn end(&mut self, end: RunEnd) {
        if self.marker.end >= Some(end) {
            return;
        }
        self.marker.end = Some(end);
        self.write();
    }

    /// Forgets a recorded panic the process outlived, such as one a worker caught. Left in
    /// place it would be read as how the run ended, and would hide a later driver fault.
    pub(super) fn survived_panic(&mut self) {
        if matches!(self.marker.end, Some(RunEnd::Panic | RunEnd::RendererPanic)) {
            self.marker.end = None;
            self.write();
        }
    }

    /// Replaces the marker file by rename.
    fn write(&mut self) {
        let temporary = self.path.with_extension("json.tmp");
        let result = serde_json::to_vec(&self.marker)
            .map_err(std::io::Error::other)
            .and_then(|bytes| fs::write(&temporary, bytes))
            .and_then(|()| fs::rename(&temporary, &self.path));
        if let Err(error) = result
            && !std::mem::replace(&mut self.write_failed, true)
        {
            tracing::warn!(%error, "could not write the renderer run marker");
        }
    }
}

/// This process's run, shared with the panic hook.
static ACTIVE: Mutex<Option<Breadcrumb>> = Mutex::new(None);

/// Set by the panic hook, so the frame loop can ask about a panic without taking the lock.
static PANICKED: AtomicBool = AtomicBool::new(false);

/// Installs `breadcrumb` as this process's run.
pub(super) fn activate(breadcrumb: Breadcrumb) {
    *ACTIVE.lock().unwrap_or_else(PoisonError::into_inner) = Some(breadcrumb);
    PANICKED.store(false, Ordering::Release);
}

/// Deletes the previous run's marker for a launch that keeps none, so a later launch does not
/// judge that old run by this launch's exit.
pub(super) fn discard(root: &Path) {
    let _ = fs::remove_file(root.join(FILE));
}

/// Applies `change` to this process's run, if a launch started one.
fn update(change: impl FnOnce(&mut Breadcrumb)) {
    if let Some(breadcrumb) = ACTIVE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        change(breadcrumb);
    }
}

/// Whether this launch keeps a run marker; callers skip their tracking without one.
pub fn is_active() -> bool {
    ACTIVE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some()
}

/// Records the renderer's stage. Call on a change, not every frame: each call takes a lock.
pub fn set_stage(stage: RunStage) {
    update(|breadcrumb| breadcrumb.stage(stage));
}

/// Records that the renderer survived this session.
pub fn mark_healthy() {
    update(Breadcrumb::healthy);
}

/// Records how this run ends; see [`RunEnd`] for which record wins.
pub fn end(end: RunEnd) {
    update(|breadcrumb| breadcrumb.end(end));
}

/// Whether a panic has been recorded and not yet outlived; one atomic read, for every frame.
pub fn panic_recorded() -> bool {
    PANICKED.load(Ordering::Acquire)
}

/// Forgets the recorded panic: the process kept drawing frames after it, so it was caught and
/// is not how the run ends.
pub fn panic_survived() {
    PANICKED.store(false, Ordering::Release);
    update(Breadcrumb::survived_panic);
}

/// Records a panic raised at source file `file`. Called from the panic hook, so it gives up
/// instead of waiting when the marker is being written; the run then ends unexplained and is
/// judged by the platform's exit record.
pub(crate) fn panicked(file: Option<&str>) {
    let end = panic_end(file);
    PANICKED.store(true, Ordering::Release);
    let mut active = match ACTIVE.try_lock() {
        Ok(active) => active,
        Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return,
    };
    if let Some(breadcrumb) = active.as_mut() {
        breadcrumb.end(end);
    }
}

/// How a panic raised at source file `file` ends the run.
fn panic_end(file: Option<&str>) -> RunEnd {
    if file.is_some_and(graphics_source) {
        RunEnd::RendererPanic
    } else {
        RunEnd::Panic
    }
}

/// Source directories whose panics are renderer failures: the GPU API, its shader
/// translator, the engine's renderer and this client's.
const GRAPHICS_SOURCES: &[&str] = &["wgpu", "naga", "bevy_render", "bevy_core_pipeline"];
/// This client's renderer crate, as its directory appears in a panic location.
const RENDER_CRATE: [&str; 2] = ["crates", "render"];

/// Whether a panic location lies in graphics code. Registry and git checkouts name their
/// directory after the crate and its version, such as `wgpu-core-30.0.1`.
fn graphics_source(file: &str) -> bool {
    let parts: Vec<&str> = file.split(['/', '\\']).collect();
    let dependency = parts.iter().any(|part| {
        GRAPHICS_SOURCES.iter().any(|name| {
            part.strip_prefix(name)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
        })
    });
    dependency
        || parts
            .windows(RENDER_CRATE.len())
            .any(|pair| pair == RENDER_CRATE)
}

#[cfg(test)]
mod tests;
