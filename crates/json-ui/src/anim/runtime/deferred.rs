//! Idle repeats of settled paints, applied only when something next uses the animator.
//!
//! Repeating a paint's bookkeeping and ending the frame touches the components it drew or
//! ticked, sampling them at the frame's time, and retires everything else. When every one of
//! them has settled, sampling moves only their sample times: no instance plays, so nothing
//! advances or fires. A run of such repeats of the same paints therefore leaves exactly what
//! the last one alone leaves, and nothing observes the animator in between unless it locks it,
//! which applies the deferred repeat first.

use super::{AnimationRecord, Animator};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// A deferred run of repeats of the same paints, at the latest frame time.
pub(super) struct DeferredRepeat {
    records: Vec<Arc<AnimationRecord>>,
    now: f64,
}

impl Animator {
    /// Defers repeating the bookkeeping of the paints recorded as `records` at `now`, followed
    /// by [`Self::end_frame`], when doing so would change nothing but their components' sample
    /// times: every component they drew or ticked exists and has settled, and every root they
    /// found destroyed still is. Whether it was deferred; when it was not, the caller repeats
    /// them itself.
    pub fn defer_repeat(&mut self, records: &[&Arc<AnimationRecord>], now: f64) -> bool {
        if let Some(deferred) = self.deferred.as_mut() {
            if deferred.records.len() == records.len()
                && deferred
                    .records
                    .iter()
                    .zip(records)
                    .all(|(deferred, record)| Arc::ptr_eq(deferred, record))
            {
                deferred.now = now;
                return true;
            }
            self.apply_deferred();
        }
        if !records.iter().all(|record| self.settled(record)) {
            return false;
        }
        self.deferred = Some(DeferredRepeat {
            records: records.iter().map(|record| Arc::clone(record)).collect(),
            now,
        });
        true
    }

    /// Whether repeating `record` would leave only sample times changed.
    fn settled(&self, record: &AnimationRecord) -> bool {
        self.can_replay(record)
            && record.ticked.iter().all(|key| {
                self.components
                    .get(key)
                    .is_some_and(|component| !component.playing())
            })
    }

    /// Applies a deferred repeat: the repeats, then the frame end they stand for.
    pub(super) fn apply_deferred(&mut self) {
        if let Some(deferred) = self.deferred.take() {
            for record in &deferred.records {
                self.replay(record, deferred.now);
            }
            self.end_frame();
        }
    }
}

/// An [`Animator`] shared behind a lock. Locking applies any repeat deferred with
/// [`Self::defer_repeat`] first, so no reader ever sees one pending.
#[derive(Default)]
pub struct SharedAnimator(Mutex<Animator>);

impl SharedAnimator {
    /// Locks the animator with any deferred repeat applied.
    pub fn lock(&self) -> MutexGuard<'_, Animator> {
        let mut animator = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        animator.apply_deferred();
        animator
    }

    /// Borrows the animator exclusively with any deferred repeat applied.
    pub fn get_mut(&mut self) -> &mut Animator {
        let animator = self.0.get_mut().unwrap_or_else(PoisonError::into_inner);
        animator.apply_deferred();
        animator
    }

    /// See [`Animator::defer_repeat`].
    pub fn defer_repeat(&self, records: &[&Arc<AnimationRecord>], now: f64) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .defer_repeat(records, now)
    }
}

#[cfg(test)]
mod tests;
