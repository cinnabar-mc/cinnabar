use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use web_time::Instant;

mod samples;
mod thread_cpu;
use samples::StageSampleAccumulator;
use thread_cpu::{ThreadCpuProfiler, ThreadCpuStart};

use bevy::prelude::{Res, ResMut, Resource};

pub mod render_counters;
mod stage;
pub use stage::RuntimeStage;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimeStageSample {
    pub count: u64,
    pub total: Duration,
    pub maximum: Duration,
    /// Number of spans with comparable calling-thread CPU counters.
    pub thread_cpu_count: u64,
    /// CPU time for those spans, including nested work.
    pub thread_cpu_total: Duration,
    pub thread_cpu_maximum: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeStageProfileSnapshot {
    pub interval: Duration,
    pub samples: [RuntimeStageSample; RuntimeStage::ALL.len()],
}

/// Creates the native lock during setup so its first timed use cannot allocate.
pub(super) fn initialized_mutex<T>(value: T) -> Mutex<T> {
    let mutex = Mutex::new(value);
    drop(
        mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    mutex
}

#[derive(Debug)]
struct RuntimeStageProfileState {
    enabled: bool,
    frame_id: AtomicU64,
    diagnostic_sink: OnceLock<DiagnosticSink>,
    thread_cpu: OnceLock<ThreadCpuProfiler>,
    slow: Option<crate::runtime_profile_slow::SlowFrameRecorder>,
    trace: Option<crate::runtime_profile_trace::FrameTrace>,
    started: Instant,
    last_snapshot_nanos: AtomicU64,
    stages: [StageSampleAccumulator; RuntimeStage::ALL.len()],
    latest_gpu: Mutex<Option<crate::GpuFrameTimes>>,
    render_counts: render_counters::RenderCountWindow,
    #[cfg(any(test, feature = "alloc-stats"))]
    allocations: crate::runtime_profile_alloc::AllocationProfiler,
}

/// Queues an owned report for background formatting without adding a crate dependency.
pub type DiagnosticSink = fn(Box<dyn std::fmt::Display + Send>);

#[derive(Resource, Debug, Clone)]
pub struct RuntimeStageProfiler {
    state: Arc<RuntimeStageProfileState>,
    /// Render-world copies keep the input identity from their extraction.
    frame_id: Option<u64>,
}

impl Default for RuntimeStageProfiler {
    fn default() -> Self {
        Self::new(false)
    }
}

impl RuntimeStageProfiler {
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self::with_trace(enabled, None)
    }

    /// Enables bounded frame spans saved at shutdown when a trace path is supplied.
    pub fn with_trace(enabled: bool, path: Option<std::path::PathBuf>) -> Self {
        Self::build(enabled, path, false)
    }

    /// Keeps bounded rolling attribution on during normal play; file exports remain opt-in.
    pub fn for_gameplay(enabled: bool, path: Option<std::path::PathBuf>) -> Self {
        Self::build(enabled, path, true)
    }

    /// Constructs aggregate profiling and the independent slow-frame recorder.
    fn build(enabled: bool, path: Option<std::path::PathBuf>, slow: bool) -> Self {
        let started = Instant::now();
        Self {
            state: Arc::new(RuntimeStageProfileState {
                enabled,
                frame_id: AtomicU64::new(0),
                diagnostic_sink: OnceLock::new(),
                thread_cpu: OnceLock::new(),
                slow: slow.then(crate::runtime_profile_slow::SlowFrameRecorder::default),
                trace: (slow || enabled && path.is_some()).then(|| {
                    crate::runtime_profile_trace::FrameTrace::new(path.filter(|_| enabled), started)
                }),
                started,
                last_snapshot_nanos: AtomicU64::new(0),
                stages: std::array::from_fn(|_| StageSampleAccumulator::default()),
                latest_gpu: initialized_mutex(None),
                render_counts: render_counters::RenderCountWindow::new(),
                #[cfg(any(test, feature = "alloc-stats"))]
                allocations: crate::runtime_profile_alloc::AllocationProfiler::default(),
            }),
            frame_id: None,
        }
    }

    /// Installs the process diagnostics queue before the profiler is shared with rendering.
    pub fn with_diagnostic_sink(self, sink: DiagnosticSink) -> Self {
        let _ = self.state.diagnostic_sink.set(sink);
        self
    }

    /// Installs an edge-owned CPU clock for opt-in aggregate profiling only.
    pub fn with_thread_cpu_clock(self, clock: fn() -> Option<Duration>) -> Self {
        if self.state.enabled {
            let _ = self.state.thread_cpu.set(ThreadCpuProfiler::new(clock));
        }
        self
    }

    /// Copies aggregate samples since the last stage-profile drain without consuming them.
    pub fn stage_samples(&self) -> Option<[RuntimeStageSample; RuntimeStage::ALL.len()]> {
        self.state
            .enabled
            .then(|| std::array::from_fn(|index| self.state.stages[index].snapshot()))
    }

    /// Current input identity, or the identity retained by an extracted render-world copy.
    pub fn frame_id(&self) -> u64 {
        self.frame_id
            .unwrap_or_else(|| self.state.frame_id.load(Ordering::Relaxed))
    }

    /// Binds a shared recorder to this input frame before render-world work starts.
    pub fn for_frame(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            frame_id: Some(self.frame_id()),
        }
    }

    /// Hands an owned snapshot to the process queue after releasing recorder locks.
    fn report(&self, report: impl std::fmt::Display + Send + 'static) {
        if let Some(sink) = self.state.diagnostic_sink.get() {
            sink(Box::new(report));
        }
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.state.enabled
    }

    /// Times a stage without allocation, retaining its input identity until it closes.
    pub fn time(&self, stage: RuntimeStage) -> RuntimeStageTimer<'_> {
        RuntimeStageTimer {
            state: self.active().then_some(&*self.state),
            stage,
            frame_id: self.frame_id(),
            started: self.active().then(Instant::now),
            cpu_started: self
                .state
                .thread_cpu
                .get()
                .and_then(ThreadCpuProfiler::start),
            #[cfg(any(test, feature = "alloc-stats"))]
            allocations: self
                .active()
                .then(crate::runtime_profile_alloc::thread_snapshot),
        }
    }

    /// Reports the preceding slow frame and starts a new attribution window.
    pub fn begin_frame(&self, focused: bool, occluded: bool) {
        crate::runtime_profile_uploads::finish_frame();
        let render_counts = self.state.render_counts.take();
        let thread_cpu = self
            .state
            .thread_cpu
            .get()
            .map(ThreadCpuProfiler::finish_frame);
        #[cfg(any(test, feature = "alloc-stats"))]
        let allocations = self.state.allocations.finish_frame();
        #[cfg(any(test, feature = "alloc-stats"))]
        allocations.plot();
        self.state.frame_id.fetch_add(1, Ordering::Relaxed);
        if let Some(slow) = &self.state.slow {
            let (event, report) = slow.begin_frame(Instant::now(), focused, occluded);
            if let Some(event) = event
                && let Some(trace) = &self.state.trace
            {
                trace.slow_frame(event);
            }
            if let Some(report) = report
                && self.state.diagnostic_sink.get().is_some()
            {
                self.report(diagnostic::SlowFrameDiagnostic {
                    timing: report,
                    thread_cpu,
                    #[cfg(any(test, feature = "alloc-stats"))]
                    allocations,
                    uploads: crate::runtime_profile_uploads::snapshot(),
                    render_counts,
                });
            }
        }
    }

    /// Sets the display interval that slow-frame thresholds and stage budgets derive from.
    pub fn set_frame_interval(&self, interval: Duration) {
        if let Some(slow) = &self.state.slow {
            slow.set_interval(interval);
        }
    }

    /// The display interval last set, or zero before the first window check.
    #[must_use]
    pub fn frame_interval(&self) -> Duration {
        self.state
            .slow
            .as_ref()
            .map_or(Duration::ZERO, |slow| slow.interval())
    }

    /// Cumulative slow frames and hitches, including those whose text was rate-limited.
    #[must_use]
    pub fn slow_frame_counts(&self) -> Option<crate::SlowFrameCounts> {
        self.state.slow.as_ref().map(|slow| slow.counts())
    }

    /// Records one read-back GPU frame, which typically trails the CPU by a few frames.
    pub fn record_gpu_frame(&self, frame: &crate::GpuFrameTimes) {
        if let Some(trace) = &self.state.trace {
            trace.gpu_frame(frame);
        }
        for (stage, elapsed) in frame.iter() {
            if self.state.enabled {
                self.state.stages[stage as usize].record(elapsed, None);
            }
            if let Some(event) = self
                .state
                .slow
                .as_ref()
                .and_then(|slow| slow.record(stage, elapsed))
                && let Some(trace) = &self.state.trace
            {
                trace.slow_frame(event);
            }
        }
        if frame.iter().any(|(stage, _)| {
            !matches!(
                stage,
                RuntimeStage::GpuFrame
                    | RuntimeStage::GpuIdle
                    | RuntimeStage::GpuFrameCompletionLatency
            )
        }) {
            *self
                .state
                .latest_gpu
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(*frame);
        }
    }

    /// The most recent read-back GPU frame that timed individual passes, if the adapter
    /// supports timestamps. Frames that timed only the whole frame leave it unchanged.
    #[must_use]
    pub fn latest_pass_frame(&self) -> Option<crate::GpuFrameTimes> {
        *self
            .state
            .latest_gpu
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Records a span measured outside a system, such as across the render handoff.
    pub fn record(&self, stage: RuntimeStage, started: Instant, elapsed: Duration) {
        if self.active() {
            record_elapsed(&self.state, self.frame_id(), stage, started, elapsed, None);
        }
    }

    /// Records accumulated worker time as a counter received on the calling thread.
    pub fn record_background_sample(&self, stage: RuntimeStage, elapsed: Duration) {
        if self.state.enabled {
            self.state.stages[stage as usize].record(elapsed, None);
        }
        if let Some(slow) = &self.state.slow {
            slow.record(stage, elapsed);
        }
        if let Some(trace) = &self.state.trace {
            trace.background_sample(stage, elapsed);
        }
    }

    /// Whether either aggregate profiling or gameplay attribution needs stage spans.
    fn active(&self) -> bool {
        self.state.enabled || self.state.slow.is_some()
    }

    /// Marks the main update boundary and its current window focus.
    pub fn trace_frame(&self, focused: bool, occluded: bool, game_seconds: f64) {
        if let Some(trace) = &self.state.trace {
            for report in trace.frame(self.frame_id(), focused, occluded, game_seconds) {
                self.report(report);
            }
        }
    }

    /// Records a workload boundary and returns its exact offset on the existing trace clock.
    pub fn trace_marker(&self, name: &'static str) -> Duration {
        if let Some(trace) = &self.state.trace {
            trace.marker(name, self.frame_id())
        } else {
            self.elapsed()
        }
    }

    /// Reads elapsed monotonic time on the same clock used by exported stage records.
    pub fn elapsed(&self) -> Duration {
        self.state.started.elapsed()
    }

    /// Starts optional trace export on a worker without blocking the update loop.
    pub fn flush_trace(&self) {
        if let Some(trace) = &self.state.trace {
            for report in trace.take_hitches(None) {
                self.report(report);
            }
            trace.flush();
        }
    }

    /// Waits for a requested export during teardown or before consuming the saved trace.
    pub fn wait_for_trace_export(&self) {
        if let Some(trace) = &self.state.trace {
            trace.wait_for_export();
        }
    }

    pub fn take_snapshot_if_due(
        &self,
        minimum_interval: Duration,
    ) -> Option<RuntimeStageProfileSnapshot> {
        if !self.state.enabled {
            return None;
        }
        let now = duration_nanos(self.state.started.elapsed());
        let minimum = duration_nanos(minimum_interval);
        let previous = self.state.last_snapshot_nanos.load(Ordering::Acquire);
        if now.saturating_sub(previous) < minimum
            || self
                .state
                .last_snapshot_nanos
                .compare_exchange(previous, now, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return None;
        }
        Some(RuntimeStageProfileSnapshot {
            interval: Duration::from_nanos(now.saturating_sub(previous)),
            samples: std::array::from_fn(|index| self.state.stages[index].take()),
        })
    }
}

/// Open spans timed across several systems, indexed by [`RuntimeStage`].
#[derive(Resource, Debug)]
pub struct RuntimeStageSpans {
    open: [Option<(Instant, u64)>; RuntimeStage::ALL.len()],
    /// Each stage's most recently closed span.
    last: [Duration; RuntimeStage::ALL.len()],
    cpu_open: [Option<ThreadCpuStart>; RuntimeStage::ALL.len()],
    cpu_last: [Option<(std::thread::ThreadId, Duration)>; RuntimeStage::ALL.len()],
    #[cfg(any(test, feature = "alloc-stats"))]
    allocations: [Option<test_alloc::Allocations>; RuntimeStage::ALL.len()],
}

#[cfg(any(test, feature = "publication-test-support"))]
impl RuntimeStageSpans {
    /// Reports whether a stage has started but has not yet closed.
    pub fn is_open(&self, stage: RuntimeStage) -> bool {
        self.open[stage as usize].is_some()
    }
}

impl Default for RuntimeStageSpans {
    fn default() -> Self {
        Self {
            open: [None; RuntimeStage::ALL.len()],
            last: [Duration::ZERO; RuntimeStage::ALL.len()],
            cpu_open: [None; RuntimeStage::ALL.len()],
            cpu_last: [None; RuntimeStage::ALL.len()],
            #[cfg(any(test, feature = "alloc-stats"))]
            allocations: [None; RuntimeStage::ALL.len()],
        }
    }
}

/// Opens the span of stage `S` (a `RuntimeStage as usize`); pair with [`end_stage_span`].
pub fn begin_stage_span<const S: usize>(
    spans: Option<ResMut<RuntimeStageSpans>>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    if let Some(mut spans) = spans {
        spans.open[S] = Some((
            Instant::now(),
            profiler.as_ref().map_or(0, |profiler| profiler.frame_id()),
        ));
        spans.cpu_open[S] = profiler
            .as_ref()
            .and_then(|profiler| profiler.state.thread_cpu.get())
            .and_then(ThreadCpuProfiler::start);
        #[cfg(any(test, feature = "alloc-stats"))]
        {
            spans.allocations[S] = Some(test_alloc::global_allocations());
        }
    }
}

/// Closes the span of stage `S` and records its wall time.
pub fn end_stage_span<const S: usize>(
    profiler: Option<Res<RuntimeStageProfiler>>,
    spans: Option<ResMut<RuntimeStageSpans>>,
) {
    let Some(mut spans) = spans else {
        return;
    };
    let Some((started, frame_id)) = spans.open[S].take() else {
        return;
    };
    let elapsed = started.elapsed();
    spans.last[S] = elapsed;
    let cpu = spans.cpu_open[S]
        .take()
        .and_then(|start| profiler.as_ref()?.state.thread_cpu.get()?.elapsed(start));
    spans.cpu_last[S] = cpu.map(|cpu| (std::thread::current().id(), cpu));
    if let Some(profiler) = profiler
        && profiler.active()
    {
        #[cfg(any(test, feature = "alloc-stats"))]
        if let Some(before) = spans.allocations[S].take() {
            profiler
                .state
                .allocations
                .record_global_since(RuntimeStage::ALL[S], before);
        }
        record_elapsed(
            &profiler.state,
            frame_id,
            RuntimeStage::ALL[S],
            started,
            elapsed,
            cpu,
        );
    }
}

/// Closes the render-frame span, excluding the drawable-acquisition wait inside it.
pub fn end_render_frame_span(
    profiler: Option<Res<RuntimeStageProfiler>>,
    spans: Option<ResMut<RuntimeStageSpans>>,
) {
    const FRAME: usize = RuntimeStage::RenderFrame as usize;
    const SURFACE: usize = RuntimeStage::SurfacePreparation as usize;
    let Some(mut spans) = spans else {
        return;
    };
    let Some((started, _)) = spans.open[FRAME].take() else {
        return;
    };
    let wait = std::mem::take(&mut spans.last[SURFACE]);
    let elapsed = render_frame_elapsed(started, Instant::now(), wait);
    spans.last[FRAME] = elapsed;
    let cpu = spans.cpu_open[FRAME].take().and_then(|start| {
        let total = profiler.as_ref()?.state.thread_cpu.get()?.elapsed(start)?;
        thread_cpu::excluding_wait(total, wait, spans.cpu_last[SURFACE].take())
    });
    if let Some(profiler) = profiler
        && profiler.active()
    {
        #[cfg(any(test, feature = "alloc-stats"))]
        if let Some(before) = spans.allocations[FRAME].take() {
            profiler
                .state
                .allocations
                .record_global_since(RuntimeStage::RenderFrame, before);
        }
        record_elapsed(
            &profiler.state,
            profiler.frame_id(),
            RuntimeStage::RenderFrame,
            started,
            elapsed,
            cpu,
        );
    }
}

#[must_use]
pub struct RuntimeStageTimer<'a> {
    state: Option<&'a RuntimeStageProfileState>,
    stage: RuntimeStage,
    frame_id: u64,
    started: Option<Instant>,
    cpu_started: Option<ThreadCpuStart>,
    #[cfg(any(test, feature = "alloc-stats"))]
    allocations: Option<crate::runtime_profile_alloc::ThreadSnapshot>,
}

impl Drop for RuntimeStageTimer<'_> {
    fn drop(&mut self) {
        if let (Some(state), Some(started)) = (self.state, self.started) {
            #[cfg(any(test, feature = "alloc-stats"))]
            if let Some(before) = self.allocations {
                state.allocations.record_since(self.stage, before);
            }
            let elapsed = started.elapsed();
            let cpu = self
                .cpu_started
                .and_then(|start| state.thread_cpu.get()?.elapsed(start));
            record_elapsed(state, self.frame_id, self.stage, started, elapsed, cpu);
        }
    }
}

/// Records aggregate timing and an optional timestamped span together.
/// Time from `started` to `now`, less the acquisition wait measured inside it.
fn render_frame_elapsed(started: Instant, now: Instant, wait: Duration) -> Duration {
    now.saturating_duration_since(started).saturating_sub(wait)
}

/// Records a completed span in each enabled diagnostic consumer with its originating frame.
fn record_elapsed(
    state: &RuntimeStageProfileState,
    frame_id: u64,
    stage: RuntimeStage,
    started: Instant,
    elapsed: Duration,
    thread_cpu: Option<Duration>,
) {
    if state.enabled {
        state.stages[stage as usize].record(elapsed, thread_cpu);
    }
    if let Some(cpu) = thread_cpu
        && let Some(recorder) = state.thread_cpu.get()
    {
        recorder.record(stage, elapsed, cpu);
    }
    if let Some(slow) = &state.slow {
        slow.record(stage, elapsed);
    }
    if let Some(trace) = &state.trace {
        trace.record_with_cpu(frame_id, stage, started, elapsed, thread_cpu);
    }
}

fn duration_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;

mod diagnostic;

#[cfg(test)]
mod allocation_tests;

#[cfg(test)]
mod thread_cpu_tests;
