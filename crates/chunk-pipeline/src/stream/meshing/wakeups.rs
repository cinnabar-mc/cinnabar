use super::super::*;

impl WorldStream {
    /// Checks coalesced dependency changes only when geometry can dispatch. Blocked keys keep
    /// their deferred owner, and unexamined notifications survive the poll's work limit.
    pub(in crate::stream) fn promote_mesh_dependency_wakeups(
        &mut self,
        view: SchedulerView,
    ) -> HashSet<SubChunkKey> {
        let mut examined = HashSet::new();
        let now = Instant::now();
        for _ in 0..MAX_PENDING_SCHEDULER_SCANS_PER_POLL {
            if !examined.is_empty() && self.poll_budget_exhausted() {
                break;
            }
            let Some(mut notification) =
                self.mesh_jobs.dependency_wakeups[RESIDENT_MESH_LANE].pop()
            else {
                break;
            };
            let Some(candidate) = self
                .mesh_jobs
                .deferred_candidate(RESIDENT_MESH_LANE, notification.key)
                .filter(|candidate| candidate.revision == notification.revision)
            else {
                continue;
            };
            notification.refresh_rank(view);
            if self.mesh_jobs.dependency_wakeups[RESIDENT_MESH_LANE]
                .peek()
                .is_some_and(|next| *next > notification)
            {
                self.mesh_jobs.dependency_wakeups[RESIDENT_MESH_LANE].push(notification);
                continue;
            }
            examined.insert(candidate.key);
            if self.mesh_retry_is_ready(candidate.key, candidate.revision, now) {
                self.mesh_jobs
                    .wake_deferred(RESIDENT_MESH_LANE, candidate.key);
            }
        }
        examined
    }
}
