use super::super::*;

impl WorldStream {
    /// Checks only dependency changes that can lead the next geometry dispatch. Blocked keys
    /// keep their deferred owner; one ready promotion returns control to actual dispatch.
    pub(in crate::stream) fn promote_mesh_dependency_wakeups(
        &mut self,
        view: SchedulerView,
        already_probed: &HashSet<SubChunkKey>,
        other_best: Option<PendingSchedulerCandidate>,
        budget: &mut usize,
    ) {
        let other_best = other_best.map(|mut candidate| {
            candidate.refresh_rank(view);
            candidate
        });
        let mut examined = false;
        let now = Instant::now();
        while *budget != 0 {
            if examined && self.poll_budget_exhausted() {
                break;
            }
            let Some(mut notification) = self.mesh_jobs.lanes[RESIDENT_MESH_LANE].notified.pop()
            else {
                break;
            };
            *budget -= 1;
            if already_probed.contains(&notification.key) {
                continue;
            }
            let Some(candidate) = self
                .mesh_jobs
                .deferred_candidate(RESIDENT_MESH_LANE, notification.key)
                .filter(|candidate| candidate.revision == notification.revision)
            else {
                continue;
            };
            notification.refresh_rank(view);
            if other_best.is_some_and(|ready| ready >= notification) {
                self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                    .notified
                    .push(notification);
                break;
            }
            if self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                .notified
                .peek()
                .is_some_and(|next| *next > notification)
            {
                self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                    .notified
                    .push(notification);
                continue;
            }
            examined = true;
            if self.mesh_retry_is_ready(candidate.key, candidate.revision, now) {
                self.mesh_jobs
                    .wake_deferred(RESIDENT_MESH_LANE, candidate.key);
                break;
            }
        }
    }
}
