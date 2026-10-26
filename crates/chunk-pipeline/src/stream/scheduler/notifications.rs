use super::*;

impl<P, J, const L: usize> KeyedJobs<P, J, L> {
    /// Reads the highest cached notification rank across both camera generations.
    pub(in crate::stream) fn peek_dependency_notification(
        &self,
        lane: usize,
    ) -> Option<PendingSchedulerCandidate> {
        self.lanes[lane]
            .notified
            .peek()
            .into_iter()
            .chain(self.refresh.previous[lane].notified.peek())
            .copied()
            .max()
    }

    /// Takes the highest cached notification without refreshing unrelated ready or deferred work.
    pub(in crate::stream) fn pop_dependency_notification(
        &mut self,
        lane: usize,
    ) -> Option<PendingSchedulerCandidate> {
        let current = &mut self.lanes[lane].notified;
        let previous = &mut self.refresh.previous[lane].notified;
        if current
            .peek()
            .is_some_and(|current| previous.peek().is_none_or(|previous| current >= previous))
        {
            current.pop()
        } else {
            previous.pop()
        }
    }

    /// Retains one reranked notification in the current generation, preserving its deferred owner.
    pub(in crate::stream) fn requeue_dependency_notification(
        &mut self,
        lane: usize,
        candidate: PendingSchedulerCandidate,
    ) {
        self.refresh.previous[lane].notified.remove(&candidate.key);
        self.lanes[lane].notified.push(candidate);
    }
}
