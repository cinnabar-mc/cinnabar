//! Storage retained only by a world worker, never by queued jobs or completed results.

use super::*;

/// Quiet interval before a worker releases temporary mesh allocations.
pub(super) const SCRATCH_IDLE_RELEASE: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(crate) struct WorkerScratch {
    pub(in crate::stream) light: world::LightSolverScratch,
    pub(in crate::stream) mesh: meshing::MeshScratch,
    finished_at: Option<Moment>,
}

impl WorkerScratch {
    /// Remaining idle grace, or no timeout when mesh storage has already been released.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn idle_wait(&self, now: Moment) -> Option<Duration> {
        self.finished_at.map(|finished| {
            SCRATCH_IDLE_RELEASE.saturating_sub(now.saturating_duration_since(finished))
        })
    }

    /// Records job completion using the executor's monotonic observation.
    pub(super) fn finished(&mut self, now: Moment) {
        self.finished_at = Some(now);
    }

    /// Releases mesh capacities after a quiet interval; lighting keeps its existing policy.
    pub(super) fn release_if_idle(&mut self, now: Moment) {
        if self
            .finished_at
            .is_some_and(|finished| now.saturating_duration_since(finished) >= SCRATCH_IDLE_RELEASE)
        {
            self.mesh = meshing::MeshScratch::default();
            self.finished_at = None;
        }
    }
}
