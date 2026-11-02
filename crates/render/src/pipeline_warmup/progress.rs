//! Names the compile work behind a pipeline wait in `client.log`, so a slow join can be attributed.

use core::time::Duration;
use std::borrow::Cow;

use bevy::render::render_resource::{
    CachedPipelineState, CachedRenderPipelineId, PipelineCache, PipelineDescriptor,
};
use bevy::shader::ShaderCacheError;
use web_time::Instant;

/// Shortest gap between progress lines; a wait with no new progress stays silent until it ends.
const REPORT_INTERVAL: Duration = Duration::from_secs(5);

/// Compile progress of the variants warm-up registered, and of the whole pipeline cache.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct WarmupCounts {
    /// Variants warm-up is tracking across all view configurations.
    pub total: usize,
    /// Tracked variants that compiled or terminally failed.
    pub finished: usize,
    /// Pipelines still compiling cache-wide, which includes actors and other non-warm-up owners.
    pub compiling: usize,
    /// Whether an owner compiling outside the cache is still busy.
    pub owned_pending: bool,
    /// Label of the first tracked variant that has not finished.
    pub next: Option<Cow<'static, str>>,
}

impl WarmupCounts {
    /// Reads the tracked states; the caller decides whether a frame needs this.
    pub(super) fn read(
        cache: &PipelineCache,
        ids: &[CachedRenderPipelineId],
        owned_pending: bool,
    ) -> Self {
        let mut counts = Self {
            total: ids.len(),
            compiling: cache.waiting_pipelines().count(),
            owned_pending,
            ..Self::default()
        };
        for &id in ids {
            if settled(cache.get_render_pipeline_state(id)) {
                counts.finished += 1;
            } else if counts.next.is_none() {
                counts.next = Some(label(cache, id));
            }
        }
        counts
    }

    fn busy(&self) -> bool {
        self.finished < self.total || self.compiling > 0 || self.owned_pending
    }
}

/// Whether a variant no longer holds loading: compiled, or failed in a way retrying cannot fix.
pub(super) fn settled(state: &CachedPipelineState) -> bool {
    match state {
        CachedPipelineState::Ok(_) => true,
        CachedPipelineState::Err(error) => !retryable(error),
        _ => false,
    }
}

/// Errors the cache retries every frame, because the shader or one of its imports is still loading.
pub(super) fn retryable(error: &ShaderCacheError) -> bool {
    matches!(
        error,
        ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable
    )
}

/// The debug label of a pipeline, including one that has not left the cache's intake yet.
fn label(cache: &PipelineCache, id: CachedRenderPipelineId) -> Cow<'static, str> {
    let descriptor = cache
        .pipelines()
        .nth(id.id())
        .map(|cached| &cached.descriptor);
    match descriptor {
        Some(PipelineDescriptor::RenderPipelineDescriptor(descriptor)) => descriptor
            .label
            .clone()
            .unwrap_or(Cow::Borrowed("unlabelled")),
        _ => Cow::Borrowed("queued"),
    }
}

/// Turns per-frame counts into rate-limited log lines covering one wait from start to finish.
#[derive(Default)]
pub(super) struct WarmupProgress {
    started: Option<Instant>,
    reported: Option<(Instant, usize, usize)>,
}

impl WarmupProgress {
    /// Whether the caller must read counts this frame: during a wait, or when one may start.
    pub(super) fn wants_counts(&self, ready: bool) -> bool {
        self.started.is_some() || !ready
    }

    /// Returns the line to log for this frame, if any. `now` comes from the render edge.
    pub(super) fn observe(
        &mut self,
        counts: &WarmupCounts,
        workers: usize,
        now: Instant,
    ) -> Option<String> {
        if !counts.busy() {
            let started = self.started.take()?;
            self.reported = None;
            return Some(format!(
                "pipeline warmup finished: {} variants after {:.1} s on {workers} compile workers",
                counts.total,
                now.duration_since(started).as_secs_f32(),
            ));
        }
        let started = *self.started.get_or_insert(now);
        let progress = (counts.finished, counts.compiling);
        let due = self.reported.is_none_or(|(at, finished, compiling)| {
            (finished, compiling) != progress && now.duration_since(at) >= REPORT_INTERVAL
        });
        if !due {
            return None;
        }
        self.reported = Some((now, progress.0, progress.1));
        Some(format!(
            "pipeline warmup waiting: {}/{} variants ready, {} pipelines compiling cache-wide, \
             owned passes pending: {}, next: {}, {:.1} s so far",
            counts.finished,
            counts.total,
            counts.compiling,
            counts.owned_pending,
            counts.next.as_deref().unwrap_or("none"),
            now.duration_since(started).as_secs_f32(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(finished: usize, compiling: usize) -> WarmupCounts {
        WarmupCounts {
            total: 10,
            finished,
            compiling,
            owned_pending: false,
            next: Some(Cow::Borrowed("terrain")),
        }
    }

    #[test]
    fn a_wait_logs_its_start_rate_limited_progress_and_its_end() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut progress = WarmupProgress::default();

        let first = progress.observe(&counts(0, 10), 2, at(0)).unwrap();
        assert!(first.contains("0/10 variants ready"), "{first}");
        assert!(first.contains("10 pipelines compiling"), "{first}");
        assert!(first.contains("next: terrain"), "{first}");
        assert_eq!(progress.observe(&counts(4, 6), 2, at(1)), None);

        let later = progress.observe(&counts(5, 5), 2, at(6)).unwrap();
        assert!(later.contains("5/10 variants ready"), "{later}");
        assert!(later.contains("6.0 s so far"), "{later}");

        let end = progress.observe(&counts(10, 0), 2, at(9)).unwrap();
        assert!(end.contains("10 variants after 9.0 s"), "{end}");
        assert!(end.contains("2 compile workers"), "{end}");
        assert_eq!(progress.observe(&counts(10, 0), 2, at(20)), None);
    }

    #[test]
    fn variants_waiting_on_shader_loads_are_not_settled() {
        let unloaded = ShaderCacheError::ShaderNotLoaded(
            bevy::prelude::Handle::<bevy::prelude::Shader>::default().id(),
        );
        assert!(!settled(&CachedPipelineState::Err(unloaded)));
        let import = ShaderCacheError::ShaderImportNotYetAvailable;
        assert!(!settled(&CachedPipelineState::Err(import)));
        assert!(!settled(&CachedPipelineState::Queued));
        let failed = ShaderCacheError::CreateShaderModule("bad".into());
        assert!(settled(&CachedPipelineState::Err(failed)));
    }

    #[test]
    fn a_stuck_wait_stays_silent_until_something_changes() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut progress = WarmupProgress::default();
        assert!(progress.observe(&counts(3, 7), 1, at(0)).is_some());
        assert_eq!(progress.observe(&counts(3, 7), 1, at(60)), None);
        assert!(progress.observe(&counts(4, 6), 1, at(61)).is_some());
    }

    #[test]
    fn idle_frames_and_a_wait_on_owned_passes_are_told_apart() {
        let now = Instant::now();
        let mut progress = WarmupProgress::default();
        let idle = WarmupCounts {
            total: 10,
            finished: 10,
            ..WarmupCounts::default()
        };
        assert_eq!(progress.observe(&idle, 1, now), None);
        assert!(!progress.wants_counts(true));
        let owned = WarmupCounts {
            owned_pending: true,
            ..idle.clone()
        };
        let line = progress.observe(&owned, 1, now).unwrap();
        assert!(line.contains("owned passes pending: true"), "{line}");
        assert!(progress.wants_counts(true));
        assert!(progress.observe(&idle, 1, now).is_some());
        assert!(!progress.wants_counts(true));
    }
}
