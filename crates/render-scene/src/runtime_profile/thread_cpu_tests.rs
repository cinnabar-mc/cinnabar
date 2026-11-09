use super::*;
use std::cell::Cell;

thread_local! {
    static CPU: Cell<Option<Duration>> = const { Cell::new(None) };
    static READS: Cell<u64> = const { Cell::new(0) };
}

/// Returns the test thread's explicitly supplied CPU counter.
fn clock() -> Option<Duration> {
    READS.with(|reads| reads.set(reads.get() + 1));
    CPU.with(Cell::get)
}

/// Sets a deterministic CPU counter without waiting for real work.
fn cpu(millis: u64) {
    CPU.with(|cpu| cpu.set(Some(Duration::from_millis(millis))));
}

/// Disabled aggregate profiling must never query the native CPU clock.
#[test]
fn cpu_sampling_is_opt_in() {
    READS.with(|reads| reads.set(0));
    let profiler = RuntimeStageProfiler::for_gameplay(false, None).with_thread_cpu_clock(clock);
    drop(profiler.time(RuntimeStage::WorldPoll));
    assert_eq!(READS.with(Cell::get), 0);
}

/// Nested spans include their children and snapshots retain both clocks without draining.
#[test]
fn nested_timers_report_thread_cpu_and_wall_samples() {
    let profiler = RuntimeStageProfiler::new(true).with_thread_cpu_clock(clock);
    cpu(10);
    let outer = profiler.time(RuntimeStage::MainFrame);
    cpu(12);
    let inner = profiler.time(RuntimeStage::UiPreparation);
    cpu(15);
    drop(inner);
    cpu(19);
    drop(outer);
    let samples = profiler.stage_samples().unwrap();
    for (stage, millis) in [
        (RuntimeStage::MainFrame, 9),
        (RuntimeStage::UiPreparation, 3),
    ] {
        let sample = samples[stage as usize];
        assert_eq!(sample.count, 1);
        assert_eq!(sample.thread_cpu_count, 1);
        assert_eq!(sample.thread_cpu_total, Duration::from_millis(millis));
        assert_eq!(sample.thread_cpu_maximum, sample.thread_cpu_total);
    }
    assert_eq!(
        profiler
            .take_snapshot_if_due(Duration::ZERO)
            .unwrap()
            .samples,
        samples
    );
}

/// Missing, failed, reversed or unrelated thread clocks never become CPU durations.
#[test]
fn incomparable_cpu_counters_preserve_wall_samples() {
    let profiler = RuntimeStageProfiler::new(true).with_thread_cpu_clock(clock);
    CPU.with(|cpu| cpu.set(None));
    drop(profiler.time(RuntimeStage::WorldPoll));
    cpu(20);
    let timer = profiler.time(RuntimeStage::WorldPoll);
    CPU.with(|cpu| cpu.set(None));
    drop(timer);
    cpu(20);
    let timer = profiler.time(RuntimeStage::WorldPoll);
    cpu(10);
    drop(timer);
    cpu(20);
    let timer = profiler.time(RuntimeStage::WorldPoll);
    std::thread::scope(|scope| {
        scope
            .spawn(move || {
                cpu(100);
                drop(timer);
            })
            .join()
            .unwrap();
    });
    let sample = profiler.stage_samples().unwrap()[RuntimeStage::WorldPoll as usize];
    assert_eq!(sample.count, 4);
    assert_eq!(sample.thread_cpu_count, 0);
}

/// Trace and spike reports retain both durations and omit unavailable CPU values.
#[test]
fn traces_and_spikes_report_cpu_only_when_measured() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("cpu.json");
    let profiler =
        RuntimeStageProfiler::for_gameplay(true, Some(path.clone())).with_thread_cpu_clock(clock);
    let started = Instant::now();
    for (stage, wall, cpu) in [
        (
            RuntimeStage::WorldPoll,
            Duration::from_millis(30),
            Some(Duration::from_millis(2)),
        ),
        (
            RuntimeStage::UiPreparation,
            Duration::from_millis(3),
            Some(Duration::from_millis(3)),
        ),
        (
            RuntimeStage::RenderSubmission,
            Duration::from_millis(4),
            None,
        ),
    ] {
        record_elapsed(&profiler.state, 1, stage, started, wall, cpu);
    }
    let frame = profiler.state.thread_cpu.get().unwrap().finish_frame();
    let mut fields = String::new();
    frame.append_fields(&mut fields);
    assert!(fields.contains("world_poll:1:30.000:2.000"), "{fields}");
    assert!(fields.contains("ui_preparation:1:3.000:3.000"), "{fields}");
    assert!(!fields.contains("render_submission"), "{fields}");
    profiler.flush_trace();
    profiler.wait_for_trace_export();
    let trace: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let events = trace["traceEvents"].as_array().unwrap();
    assert_eq!(events[0]["dur"], 30_000.0);
    assert_eq!(events[0]["args"]["thread_cpu_ns"], 2_000_000);
    assert_eq!(events[1]["args"]["thread_cpu_ns"], 3_000_000);
    assert!(events[2]["args"].get("thread_cpu_ns").is_none());
}

/// Diagnostic totals saturate instead of wrapping, and absent clocks emit wall time alone.
#[test]
fn cpu_totals_saturate_and_unsupported_clocks_emit_no_fields() {
    let accumulator = StageSampleAccumulator::default();
    accumulator.record(Duration::MAX, Some(Duration::MAX));
    accumulator.record(Duration::MAX, Some(Duration::MAX));
    let sample = accumulator.take();
    assert_eq!(sample.total, Duration::MAX);
    assert_eq!(sample.thread_cpu_total, Duration::MAX);
    let profiler = RuntimeStageProfiler::for_gameplay(true, None).with_thread_cpu_clock(clock);
    CPU.with(|cpu| cpu.set(None));
    drop(profiler.time(RuntimeStage::WorldPoll));
    let mut fields = String::new();
    profiler
        .state
        .thread_cpu
        .get()
        .unwrap()
        .finish_frame()
        .append_fields(&mut fields);
    assert!(fields.is_empty());
    assert_eq!(
        profiler.stage_samples().unwrap()[RuntimeStage::WorldPoll as usize].count,
        1
    );
}

/// Schedule boundaries sample CPU clocks and remove acquisition work from render totals.
#[test]
fn schedule_spans_exclude_the_surface_wait_from_both_clocks() {
    use bevy::{ecs::system::SystemState, prelude::*};
    const RENDER: usize = RuntimeStage::RenderFrame as usize;
    const SURFACE: usize = RuntimeStage::SurfacePreparation as usize;
    let profiler = RuntimeStageProfiler::new(true).with_thread_cpu_clock(clock);
    let mut world = World::new();
    world.insert_resource(profiler.clone());
    world.init_resource::<RuntimeStageSpans>();
    let mut parameters = SystemState::<(
        Option<Res<RuntimeStageProfiler>>,
        Option<ResMut<RuntimeStageSpans>>,
    )>::new(&mut world);
    cpu(10);
    let (profile, spans) = parameters.get_mut(&mut world).unwrap();
    begin_stage_span::<RENDER>(spans, profile);
    cpu(12);
    let (profile, spans) = parameters.get_mut(&mut world).unwrap();
    begin_stage_span::<SURFACE>(spans, profile);
    cpu(15);
    let (profile, spans) = parameters.get_mut(&mut world).unwrap();
    end_stage_span::<SURFACE>(profile, spans);
    // Select the wait-exclusion branch without relying on native clock resolution.
    world.resource_mut::<RuntimeStageSpans>().last[SURFACE] = Duration::from_millis(1);
    cpu(20);
    let (profile, spans) = parameters.get_mut(&mut world).unwrap();
    end_render_frame_span(profile, spans);
    let samples = profiler.stage_samples().unwrap();
    assert_eq!(samples[SURFACE].thread_cpu_total, Duration::from_millis(3));
    assert_eq!(samples[RENDER].thread_cpu_total, Duration::from_millis(7));
    assert_eq!(samples[RENDER].thread_cpu_count, 1);
}

/// Missing or unrelated acquisition counters cannot be subtracted from render CPU time.
#[test]
fn excluded_cpu_wait_requires_a_comparable_counter() {
    use thread_cpu::excluding_wait;
    let total = Duration::from_millis(5);
    assert_eq!(excluding_wait(total, Duration::ZERO, None), Some(total));
    assert_eq!(excluding_wait(total, Duration::from_millis(1), None), None);
    let other = std::thread::spawn(|| std::thread::current().id())
        .join()
        .unwrap();
    assert_eq!(
        excluding_wait(
            total,
            Duration::from_millis(1),
            Some((other, Duration::ZERO))
        ),
        None
    );
    assert_eq!(
        excluding_wait(
            total,
            Duration::from_millis(1),
            Some((std::thread::current().id(), Duration::from_millis(6)))
        ),
        None
    );
}
