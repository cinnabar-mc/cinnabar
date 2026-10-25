use super::super::*;

#[cfg(test)]
thread_local! {
    pub(in crate::stream) static MESH_READINESS_CHECKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl WorldStream {
    /// Offers bounded FIFO retries not already probed, without adding heap records.
    pub(in crate::stream) fn mesh_retry_candidates(
        &mut self,
        view: SchedulerView,
        already_probed: &HashSet<SubChunkKey>,
    ) -> Vec<(PendingSchedulerCandidate, PendingMesh, MeshCandidateOrigin)> {
        let mut candidates = Vec::new();
        let budget = self
            .mesh_jobs
            .retries
            .begin_poll()
            .min(MAX_PENDING_SCHEDULER_SCANS_PER_POLL);
        let now = Instant::now();
        let mut examined = false;
        for _ in 0..budget {
            if examined && self.poll_budget_exhausted() {
                break;
            }
            let Some(retry) = self.mesh_jobs.retries.pop() else {
                break;
            };
            let Some(pending) = self
                .mesh_jobs
                .pending
                .get(&retry.key)
                .copied()
                .filter(|pending| pending.revision == retry.revision)
            else {
                continue;
            };
            if !self.mesh_requires_geometry(retry.key) {
                continue;
            }
            if already_probed.contains(&retry.key) {
                self.mesh_jobs.retries.requeue(retry);
                continue;
            }
            if self.mesh_jobs.has_dependency_notification(
                RESIDENT_MESH_LANE,
                retry.key,
                retry.revision,
            ) {
                self.mesh_jobs.retries.requeue(retry);
                continue;
            }
            if !self.mesh_jobs.retries.is_due(&retry) {
                self.mesh_jobs.retries.requeue(retry);
                continue;
            }
            let candidate =
                PendingSchedulerCandidate::new(retry.key, retry.revision, view, pending.urgent);
            let ready_leads = self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                .ready
                .peek()
                .copied()
                .filter(|ready| {
                    self.mesh_jobs
                        .pending
                        .get(&ready.key)
                        .is_some_and(|pending| pending.revision == ready.revision)
                })
                .is_some_and(|mut ready| {
                    ready.refresh_rank(view);
                    ready >= candidate
                });
            if ready_leads {
                self.mesh_jobs.retries.requeue(retry);
                continue;
            }
            examined = true;
            // Output credit is part of readiness; dispatch reserves it again in rank order.
            let ready = self.mesh_retry_is_ready(retry.key, retry.revision, now);
            if ready {
                candidates.push((candidate, pending, MeshCandidateOrigin::Retry));
                self.mesh_jobs.retries.requeue(retry);
            } else {
                self.mesh_jobs.retries.defer(retry);
            }
        }
        candidates
    }

    /// Offers deferred geometry only when its inputs and output reservation are available.
    pub(in crate::stream) fn mesh_retry_is_ready(
        &mut self,
        key: SubChunkKey,
        revision: u64,
        now: Instant,
    ) -> bool {
        #[cfg(test)]
        MESH_READINESS_CHECKS.with(|checks| checks.set(checks.get() + 1));
        self.revisions.is_current(key, revision)
            && !self.mesh_jobs.in_flight.contains_key(&key)
            && !self.mesh_neighbour_is_due(key, now)
            && self.mesh_light_halo_is_ready(key)
            && self
                .authority
                .terrain()
                .sub_chunk(key)
                .is_some_and(|source| {
                    self.mesh_memory
                        .try_admit(
                            &source,
                            self.authority.runtime_assets(),
                            self.authority.network_id_mode(),
                            self.overlay_hosts(key).count(),
                        )
                        .is_some()
                })
    }
}
