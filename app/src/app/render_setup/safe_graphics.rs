//! Chooses this launch's safe graphics level, tells the player about it and keeps the run
//! marker current, so the next launch knows where this one's renderer ended.

use std::{path::Path, time::Duration};

use bevy::{
    app::OnAppExitSystems,
    prelude::{
        App, AppExit, IntoScheduleConfigs, Last, Local, MessageReader, Real, Res, Time, Update,
    },
};
use launcher::graphics_backend::Platform;
use launcher_host::{
    lifecycle::crash::state::SessionPhase,
    renderer_startup::{
        breadcrumb::{self, RunEnd, RunScope, RunStage},
        exit_record,
        safe_mode::{self, Launch, SafeStart},
    },
};
use render_api::SafeGraphics;

use super::device::log;

/// How long the renderer must draw a world before the session counts as one it survived.
/// The reported driver faults come within seconds of the menu or of entering a world; the
/// slowest ones lose the device after several minutes, and those still count as failures
/// because a later GPU error outranks this mark.
pub(super) const HEALTHY_WORLD_TIME: Duration = Duration::from_secs(120);

/// The most real time one frame may add to a count. The real clock keeps running while the
/// app is suspended, and that time proves nothing about the renderer.
pub(super) const MAX_FRAME_CREDIT: Duration = Duration::from_secs(1);

/// How long frames must keep coming after a panic before it counts as caught. An uncaught
/// one stops the frame loop at once, or within a frame when another thread raised it.
pub(super) const PANIC_SURVIVAL_TIME: Duration = Duration::from_secs(5);

/// Chooses the level before any plugin is built, logs it with the reason and starts the run
/// marker. Only platforms whose policy steps down within one graphics API keep a ladder;
/// [`SafeGraphics::ENV`] forces a level anywhere and saves nothing.
///
/// The caller holds the returned scope until the launch returns: dropping it ends the run
/// marker, so no return path, such as a failed prerequisite, leaves a run that looks killed.
pub(crate) fn start(data_root: &Path, graphics_api_revision: u64) -> (SafeStart, RunScope) {
    (choose(data_root, graphics_api_revision), RunScope)
}

/// Chooses and logs the level; see [`start`].
fn choose(data_root: &Path, graphics_api_revision: u64) -> SafeStart {
    let forced = std::env::var(SafeGraphics::ENV)
        .ok()
        .and_then(|value| SafeGraphics::parse(&value));
    let start = if let Some(level) = forced {
        SafeStart::forced(data_root, level)
    } else if Platform::current().steps_down_graphics() {
        safe_mode::start(
            data_root,
            Launch {
                version: launcher::PRODUCT_VERSION,
                graphics_api_revision,
                exit: exit_record::previous(),
                now_ms: unix_millis(),
            },
        )
    } else {
        return SafeStart::unused();
    };
    log(format_args!(
        "Renderer safe graphics level {} of {} ({}): {}",
        start.level.level(),
        SafeGraphics::MAX.level(),
        start.level.summary(),
        start.reason
    ));
    start
}

/// Now, in milliseconds since the Unix epoch, as Android stamps its exit records.
fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Queues the player's notice and, when this launch keeps a run marker, the systems that
/// follow the run.
pub(super) fn install(app: &mut App, start: &SafeStart) {
    if start.notify
        && let Some(message) =
            launcher::graphics_backend::safe_graphics_notice(start.level, start.kept)
    {
        super::guarded::install_notice(app, message);
    }
    if breadcrumb::is_active() {
        app.add_systems(Update, track_run)
            .add_systems(Last, record_exit.in_set(OnAppExitSystems));
    }
}

/// What one frame shows about the run.
#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    /// A frame has finished its GPU work and reached the display.
    pub presented: bool,
    pub phase: SessionPhase,
    /// World pipelines are admitted and not all compiled.
    pub warming: bool,
    /// The renderer stopped on a GPU error.
    pub gpu_failed: bool,
    /// The marker holds a panic the process has not yet outlived.
    pub panicked: bool,
}

/// The run's stage for a frame.
pub(super) fn stage(frame: Frame) -> RunStage {
    match frame.phase {
        _ if !frame.presented => RunStage::Startup,
        SessionPhase::InWorld => RunStage::InWorld,
        SessionPhase::Joining => RunStage::Joining,
        SessionPhase::Menus if frame.warming => RunStage::WarmUp,
        SessionPhase::Menus => RunStage::Menu,
    }
}

/// What a frame changed and the marker must record.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Changes {
    pub stage: Option<RunStage>,
    pub healthy: bool,
    pub gpu_failed: bool,
    /// Frames kept coming after a recorded panic, so it was caught.
    pub panic_survived: bool,
}

/// Follows a run frame by frame and reports only what changed, so the marker is written on
/// transitions and never per frame.
#[derive(Default)]
pub(super) struct RunTracker {
    stage: Option<RunStage>,
    in_world: Duration,
    healthy: bool,
    gpu_failed: bool,
    /// Frame time since a recorded panic.
    since_panic: Duration,
}

impl RunTracker {
    /// Advances by one frame that took `elapsed` of real time, of which at most
    /// [`MAX_FRAME_CREDIT`] counts.
    pub(super) fn observe(&mut self, frame: Frame, elapsed: Duration) -> Changes {
        let elapsed = elapsed.min(MAX_FRAME_CREDIT);
        let mut changes = Changes::default();
        if frame.panicked {
            self.since_panic += elapsed;
            if self.since_panic >= PANIC_SURVIVAL_TIME {
                self.since_panic = Duration::ZERO;
                changes.panic_survived = true;
            }
        } else {
            self.since_panic = Duration::ZERO;
        }
        let stage = stage(frame);
        if self.stage != Some(stage) {
            self.stage = Some(stage);
            // A new run already records startup.
            changes.stage = (stage != RunStage::Startup).then_some(stage);
        }
        if frame.gpu_failed && !std::mem::replace(&mut self.gpu_failed, true) {
            changes.gpu_failed = true;
        }
        if stage == RunStage::InWorld && !self.gpu_failed && !self.healthy {
            self.in_world += elapsed;
            if self.in_world >= HEALTHY_WORLD_TIME {
                self.healthy = true;
                changes.healthy = true;
            }
        }
        changes
    }
}

/// Records stage changes, a GPU failure and the healthy mark in the run marker.
fn track_run(
    presentation: Res<render_scene::startup_presentation::StartupPresentation>,
    admission: Option<Res<render_scene::startup_work::WorldWorkAdmission>>,
    warmup: Option<Res<render_scene::PipelineWarmupReadiness>>,
    failure: Option<Res<render::GpuFailure>>,
    time: Res<Time<Real>>,
    mut tracker: Local<RunTracker>,
) {
    let frame = Frame {
        presented: presentation.has_presented(),
        phase: launcher_host::lifecycle::crash::state::game().phase,
        warming: admission
            .is_some_and(|admission| admission.prepares_in_background() && admission.is_admitted())
            && warmup.is_some_and(|warmup| !warmup.is_ready()),
        gpu_failed: failure.is_some_and(|failure| failure.message().is_some()),
        panicked: breadcrumb::panic_recorded(),
    };
    let changes = tracker.observe(frame, time.delta());
    if changes.panic_survived {
        breadcrumb::panic_survived();
    }
    if changes.gpu_failed {
        breadcrumb::end(RunEnd::GpuFailure);
    }
    if let Some(stage) = changes.stage {
        breadcrumb::set_stage(stage);
    }
    if changes.healthy {
        breadcrumb::mark_healthy();
    }
}

/// Records an orderly exit; a GPU failure that caused it keeps the stronger record.
fn record_exit(mut exits: MessageReader<AppExit>, failure: Option<Res<render::GpuFailure>>) {
    if exits.read().next().is_none() {
        return;
    }
    if failure.is_some_and(|failure| failure.message().is_some()) {
        breadcrumb::end(RunEnd::GpuFailure);
    }
    breadcrumb::end(RunEnd::Clean);
}

#[cfg(test)]
mod tests;
