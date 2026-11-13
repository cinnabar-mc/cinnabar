//! Owned slow-frame fields, formatted after the frame thread releases its counters.

use crate::{runtime_profile_slow::SlowFrameReport, runtime_profile_uploads::UploadFrame};

pub(super) struct SlowFrameDiagnostic {
    pub timing: SlowFrameReport,
    pub thread_cpu: Option<super::thread_cpu::ThreadCpuFrame>,
    #[cfg(any(test, feature = "alloc-stats"))]
    pub allocations: crate::runtime_profile_alloc::AllocationFrame,
    pub uploads: UploadFrame,
    /// Work counted by the render frames that closed during the reported interval.
    pub render_counts: super::render_counters::RenderCounts,
}

impl std::fmt::Display for SlowFrameDiagnostic {
    /// Formats captured intervals without reading the live timing or upload tables.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.timing.fmt(f)?;
        let mut fields = String::new();
        if let Some(cpu) = &self.thread_cpu {
            cpu.append_fields(&mut fields);
        }
        #[cfg(any(test, feature = "alloc-stats"))]
        self.allocations.append_fields(&mut fields);
        self.uploads.append_fields(&mut fields);
        self.render_counts.append_fields(&mut fields);
        f.write_str(&fields)
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    thread_local! {
        static REPORTS: std::cell::RefCell<Vec<Box<dyn std::fmt::Display + Send>>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    /// Retains the owned report until the test explicitly formats it.
    fn retain_report(report: Box<dyn std::fmt::Display + Send>) {
        REPORTS.with_borrow_mut(|reports| reports.push(report));
    }

    /// Later frame drains cannot change the allocation fields queued with a slow frame.
    #[test]
    fn deferred_report_retains_the_completed_allocation_interval() {
        let profiler =
            RuntimeStageProfiler::for_gameplay(false, None).with_diagnostic_sink(retain_report);
        profiler.begin_frame(true, false);
        profiler.state.allocations.record(
            RuntimeStage::UiPreparation,
            test_alloc::Allocations {
                calls: 2,
                bytes: 96,
            },
        );
        profiler.record(
            RuntimeStage::MainFrame,
            Instant::now(),
            Duration::from_millis(30),
        );
        profiler.begin_frame(true, false);
        profiler.begin_frame(true, false);
        let reports = REPORTS.with_borrow_mut(std::mem::take);
        assert_eq!(reports.len(), 1);
        let line = reports[0].to_string();
        assert!(line.contains("CINNABAR_SLOW_FRAME frame=1 "), "{line}");
        assert!(
            line.contains("stage_allocations=ui_preparation:2:96 "),
            "{line}"
        );
    }

    /// Render-thread stages and the render frame's work counters reach the slow-frame line.
    #[test]
    fn slow_frame_report_carries_render_stages_and_counters() {
        use render_counters::{RenderCounter, count};
        let stages = [
            RuntimeStage::RenderGraph,
            RuntimeStage::NodeOpaque,
            RuntimeStage::RenderReadback,
            RuntimeStage::EncodeFinish,
            RuntimeStage::QueueSubmit,
            RuntimeStage::SurfacePresent,
            RuntimeStage::UpdateUiAuthority,
            RuntimeStage::PostVisibility,
        ];
        let profiler =
            RuntimeStageProfiler::for_gameplay(false, None).with_diagnostic_sink(retain_report);
        profiler.begin_frame(true, false);
        for stage in stages {
            profiler.record(stage, Instant::now(), Duration::from_millis(2));
        }
        count(RenderCounter::Passes, 3);
        count(RenderCounter::Pipelines, 1);
        count(RenderCounter::WriteBytes, 4096);
        let frame = profiler.finish_render_counters();
        assert!(frame.get(RenderCounter::Passes) >= 3);
        profiler.record(
            RuntimeStage::MainFrame,
            Instant::now(),
            Duration::from_millis(30),
        );
        profiler.begin_frame(true, false);
        let reports = REPORTS.with_borrow_mut(std::mem::take);
        assert_eq!(reports.len(), 1);
        let line = reports[0].to_string();
        for stage in stages {
            assert!(line.contains(&format!("{}:", stage.name())), "{line}");
        }
        let counts = line
            .split(" render_counts=")
            .nth(1)
            .and_then(|fields| fields.split(' ').next())
            .unwrap_or_else(|| panic!("no render counters: {line}"));
        let reported = |counter: RenderCounter| {
            counts
                .split(',')
                .find_map(|field| field.strip_prefix(&format!("{}:", counter.name())))
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or_else(|| panic!("no {} in {counts}", counter.name()))
        };
        assert!(reported(RenderCounter::Passes) >= 3, "{counts}");
        assert!(reported(RenderCounter::Pipelines) >= 1, "{counts}");
        assert!(reported(RenderCounter::WriteBytes) >= 4096, "{counts}");
    }

    /// Deferred slow-frame lines preserve paired CPU and wall durations.
    #[test]
    fn deferred_slow_frame_report_includes_measured_thread_cpu() {
        let profiler = RuntimeStageProfiler::for_gameplay(true, None)
            .with_diagnostic_sink(retain_report)
            .with_thread_cpu_clock(|| Some(Duration::ZERO));
        profiler.begin_frame(true, false);
        record_elapsed(
            &profiler.state,
            profiler.frame_id(),
            RuntimeStage::MainFrame,
            Instant::now(),
            Duration::from_millis(30),
            Some(Duration::from_millis(2)),
        );
        profiler.begin_frame(true, false);
        let reports = REPORTS.with_borrow_mut(std::mem::take);
        assert_eq!(reports.len(), 1);
        let line = reports[0].to_string();
        assert!(line.contains("CINNABAR_SLOW_FRAME"), "{line}");
        assert!(
            line.contains("stage_thread_cpu=main_frame:1:30.000:2.000"),
            "{line}"
        );
    }
}
