use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowOccluded};
use std::time::Duration;
#[cfg(feature = "tracy")]
use std::time::Instant;
use {
    render::FramePacing,
    render_scene::{FrameBudgets, RuntimeStageProfiler},
};

use crate::present_mode::DisplayRefresh;

/// Builds rolling attribution and sends owned reports to the background console.
pub(crate) fn profiler(
    enabled: bool,
    trace_path: Option<std::path::PathBuf>,
) -> RuntimeStageProfiler {
    RuntimeStageProfiler::for_gameplay(enabled, trace_path)
        .with_diagnostic_sink(diagnostics::console::defer_stderr)
        .with_thread_cpu_clock(diagnostics::thread_cpu::thread_cpu_time)
}

/// Feeds slow-frame thresholds the primary window's monitor interval, or a slower frame cap.
pub(crate) fn track_frame_interval(
    profiler: Res<RuntimeStageProfiler>,
    display: Option<Res<DisplayRefresh>>,
    pacing: Option<Res<FramePacing>>,
) {
    if !display.as_ref().is_some_and(|display| display.is_changed())
        && !pacing.as_ref().is_some_and(|pacing| pacing.is_changed())
    {
        return;
    }
    let refresh = display
        .and_then(|display| display.0.refresh)
        .map(|rate| rate.millihertz());
    let cap = pacing
        .and_then(|pacing| pacing.rate)
        .map(|rate| Duration::from_nanos(rate.period_nanos()));
    profiler.set_frame_interval(FrameBudgets::display_interval(refresh, cap));
}

/// Records focus and OS occlusion with the optional bounded frame trace.
pub(crate) fn trace_frame_focus(
    time: Res<Time>,
    profiler: Res<RuntimeStageProfiler>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut events: MessageReader<WindowOccluded>,
    mut occluded: Local<bool>,
    #[cfg(feature = "tracy")] mut anchor: Local<Option<Instant>>,
) {
    #[cfg(feature = "tracy")]
    if anchor.is_none_or(|last| last.elapsed() >= Duration::from_secs(1)) {
        *anchor = Some(Instant::now());
        let _window = bevy::log::info_span!("profile.clock_window").entered();
        let unix_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let _zone = bevy::log::info_span!(
            "profile.clock_anchor",
            unix_ns,
            game_seconds = time.elapsed_secs_f64()
        )
        .entered();
    }
    for event in events.read() {
        *occluded = event.occluded;
    }
    let focused = windows.single().is_ok_and(|window| window.focused);
    profiler.begin_frame(focused, *occluded);
    profiler.trace_frame(focused, *occluded, time.elapsed_secs_f64());
}

/// Starts trace export on the exit frame, before runner teardown can retain resources.
pub(crate) fn flush_trace_on_exit(
    profiler: Res<RuntimeStageProfiler>,
    mut exit: MessageReader<AppExit>,
) {
    if exit.read().next().is_some() {
        profiler.flush_trace();
        client_runtime::shutdown::log_shutdown_phase("trace_flush");
    }
}

/// Completes export when native termination clears the world without returning from the runner.
#[derive(Resource)]
struct TraceExportOnTeardown(RuntimeStageProfiler);

impl Drop for TraceExportOnTeardown {
    /// Flushes and joins even while the runner's profiler handle remains alive.
    fn drop(&mut self) {
        self.0.flush_trace();
        self.0.wait_for_trace_export();
    }
}

/// Splits `main_update` along the client frame-set chain. Each stage runs from its first set
/// to the next stage's, so a system the schedule places between two boundaries counts toward
/// that stage; `main_update` beyond these stages ran outside the chain.
pub(crate) fn install_frame_set_spans(app: &mut App) {
    use client_state::ClientFrameSet as Set;
    use render_scene::{RuntimeStage, begin_stage_span, end_stage_span};
    const INPUT: usize = RuntimeStage::UpdateInput as usize;
    const UI_AUTHORITY: usize = RuntimeStage::UpdateUiAuthority as usize;
    const SIMULATION: usize = RuntimeStage::UpdateSimulation as usize;
    const NETWORK_SEND: usize = RuntimeStage::UpdateNetworkSend as usize;
    const WORLD: usize = RuntimeStage::UpdateWorld as usize;
    const PRESENTATION: usize = RuntimeStage::UpdatePresentation as usize;
    app.init_resource::<render_scene::RuntimeStageSpans>()
        .add_systems(
            Update,
            (
                begin_stage_span::<INPUT>.before(Set::RawInput),
                (end_stage_span::<INPUT>, begin_stage_span::<UI_AUTHORITY>)
                    .chain()
                    .after(Set::SemanticSample)
                    .before(Set::UiAuthority),
                (
                    end_stage_span::<UI_AUTHORITY>,
                    begin_stage_span::<SIMULATION>,
                )
                    .chain()
                    .after(Set::SemanticFinalize)
                    .before(Set::Physics),
                (
                    end_stage_span::<SIMULATION>,
                    begin_stage_span::<NETWORK_SEND>,
                )
                    .chain()
                    .after(Set::Interaction)
                    .before(Set::NetworkSend),
                (end_stage_span::<NETWORK_SEND>, begin_stage_span::<WORLD>)
                    .chain()
                    .after(Set::NetworkSend)
                    .before(Set::WorldPublication),
                (end_stage_span::<WORLD>, begin_stage_span::<PRESENTATION>)
                    .chain()
                    .after(Set::WorldPublication)
                    .before(Set::ActorPreparation),
                end_stage_span::<PRESENTATION>.after(Set::UiPublication),
            ),
        );
}

/// Completes export during world teardown or after a runner retains its world and returns.
pub(crate) fn run(app: &mut App) -> AppExit {
    let profiler = app.world().resource::<RuntimeStageProfiler>().clone();
    // Like TeardownWatchdog, install before runner cleanup adds blocking render teardown resources.
    app.insert_resource(TraceExportOnTeardown(profiler.clone()));
    let exit = app.run();
    client_runtime::shutdown::log_shutdown_phase("app_run");
    profiler.flush_trace();
    profiler.wait_for_trace_export();
    client_runtime::shutdown::log_shutdown_phase("trace_export");
    exit
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Native teardown must finish export before the runner returns, without an exit message.
    #[test]
    fn trace_export_finishes_during_world_teardown_before_runner_returns() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("teardown.json");
        let profiler = RuntimeStageProfiler::with_trace(true, Some(path.clone()));
        profiler.begin_frame(true, false);
        profiler.trace_frame(true, false, 0.0);
        let mut app = App::new();
        app.insert_resource(profiler);
        app.set_runner(move |mut owned| {
            assert!(owned.should_exit().is_none());
            owned.world_mut().clear_all();
            let saved: serde_json::Value = serde_json::from_slice(
                &std::fs::read(&path).expect("trace exported during teardown"),
            )
            .unwrap();
            assert_eq!(saved["traceEvents"][0]["frame_id"], 1);
            AppExit::Success
        });
        assert!(run(&mut app).is_success());
    }

    /// A runner retaining its world cannot leave an unfinished export at process exit.
    #[test]
    fn trace_export_finishes_after_a_runner_retains_the_world() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("shutdown.json");
        let profiler = RuntimeStageProfiler::with_trace(true, Some(path.clone()));
        profiler.begin_frame(true, false);
        profiler.trace_frame(true, false, 0.0);
        let retained = std::rc::Rc::new(std::cell::RefCell::new(None));
        let runner_app = retained.clone();
        let mut app = App::new();
        app.insert_resource(profiler);
        app.set_runner(move |app| {
            *runner_app.borrow_mut() = Some(app);
            AppExit::Success
        });
        assert!(run(&mut app).is_success());
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["traceEvents"][0]["frame_id"], 1);
        assert!(retained.borrow().is_some());
    }

    /// Records a zero-length probe span from inside a frame set.
    fn probe<const S: usize>(profiler: Res<RuntimeStageProfiler>) {
        drop(profiler.time(render_scene::RuntimeStage::ALL[S]));
    }

    /// Work in a frame set is attributed to the update stage covering that set, once a frame.
    #[test]
    fn frame_set_stages_enclose_the_sets_they_cover() {
        use client_state::ClientFrameSet as Set;
        use render_scene::RuntimeStage;
        const IN_UI_AUTHORITY: usize = RuntimeStage::Particles as usize;
        const IN_NETWORK_SEND: usize = RuntimeStage::Audio as usize;
        const IN_UI_PUBLICATION: usize = RuntimeStage::BlockEntities as usize;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("frame-sets.json");
        let profiler = RuntimeStageProfiler::with_trace(true, Some(path.clone()));
        let mut app = App::new();
        app.insert_resource(profiler.clone());
        crate::game_frame::configure_client_frame_schedule(&mut app);
        install_frame_set_spans(&mut app);
        app.add_systems(
            Update,
            (
                probe::<IN_UI_AUTHORITY>.in_set(Set::UiAuthority),
                probe::<IN_NETWORK_SEND>.in_set(Set::NetworkSend),
                probe::<IN_UI_PUBLICATION>.in_set(Set::UiPublication),
            ),
        );
        app.update();
        let stages = [
            RuntimeStage::UpdateInput,
            RuntimeStage::UpdateUiAuthority,
            RuntimeStage::UpdateSimulation,
            RuntimeStage::UpdateNetworkSend,
            RuntimeStage::UpdateWorld,
            RuntimeStage::UpdatePresentation,
        ];
        let snapshot = profiler.take_snapshot_if_due(Duration::ZERO).unwrap();
        for stage in stages {
            assert_eq!(
                snapshot.samples[stage as usize].count,
                1,
                "{}",
                stage.name()
            );
        }
        profiler.flush_trace();
        profiler.wait_for_trace_export();
        let trace: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let span = |name: &str| {
            let event = trace["traceEvents"]
                .as_array()
                .unwrap()
                .iter()
                .find(|event| event["name"] == name)
                .unwrap_or_else(|| panic!("missing span {name}"));
            let start = event["ts"].as_f64().unwrap();
            (start, start + event["dur"].as_f64().unwrap())
        };
        for (probe, owner) in [
            (IN_UI_AUTHORITY, RuntimeStage::UpdateUiAuthority),
            (IN_NETWORK_SEND, RuntimeStage::UpdateNetworkSend),
            (IN_UI_PUBLICATION, RuntimeStage::UpdatePresentation),
        ] {
            let (start, end) = span(RuntimeStage::ALL[probe].name());
            let enclosing: Vec<_> = stages
                .into_iter()
                .filter(|stage| {
                    let (low, high) = span(stage.name());
                    low <= start && end <= high
                })
                .collect();
            assert_eq!(enclosing, [owner]);
        }
    }
}
