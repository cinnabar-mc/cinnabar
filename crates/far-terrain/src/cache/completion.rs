//! Ordered, resumable application of disk results on the cache owner.

use std::ops::Bound::{Excluded, Unbounded};

use super::*;
use crate::{config::CACHE_EVENTS_PER_UPDATE, disk::DiskEvent};

pub(super) enum PendingEvent {
    Loaded {
        namespace: NamespaceKey,
        region: RegionPos,
        columns: std::vec::IntoIter<(ColumnPos, Versioned)>,
        after: Option<ColumnPos>,
    },
    Written {
        namespace: NamespaceKey,
        region: RegionPos,
        columns: std::vec::IntoIter<(ColumnPos, u64)>,
    },
}

impl FarTerrainCache {
    /// Starts only a bounded number of events and finishes each before reading another.
    pub(super) fn apply_completions(&mut self, now: Option<Instant>, budget: &mut SummaryBudget) {
        for _ in 0..CACHE_EVENTS_PER_UPDATE {
            if let Some(namespace) = self.namespace {
                while let Some((_, column)) = self.unresolved.last() {
                    if !budget.take(column.summary.bytes.len()) {
                        return;
                    }
                    let (pos, column) = self.unresolved.last().cloned().expect("checked above");
                    if !self.try_queue_write(namespace, pos, &column, now) {
                        break;
                    }
                    self.unresolved.pop();
                }
            }
            if let Some(mut pending) = self.pending_event.take()
                && !self.apply_event(&mut pending, budget)
            {
                self.pending_event = Some(pending);
                return;
            }
            let Some(event) = self.disk.as_ref().and_then(|disk| disk.events().next()) else {
                return;
            };
            self.receive_event(event);
        }
    }

    /// Stages one IO result after the preceding result has been fully applied.
    pub(super) fn receive_event(&mut self, event: DiskEvent) {
        debug_assert!(self.pending_event.is_none());
        match event {
            DiskEvent::Resolved {
                world,
                entry,
                epoch,
            } => {
                if entry == self.entry {
                    self.resolved(world, epoch);
                }
            }
            DiskEvent::Loaded {
                namespace,
                region,
                columns,
            } => {
                self.pending_event = Some(PendingEvent::Loaded {
                    namespace,
                    region,
                    columns: columns.into_iter(),
                    after: None,
                });
            }
            DiskEvent::Written {
                namespace,
                region,
                columns,
            } => {
                self.pending_event = Some(PendingEvent::Written {
                    namespace,
                    region,
                    columns: columns.into_iter(),
                });
            }
        }
    }

    /// Applies columns individually, including queued versions absent from a region read.
    pub(super) fn apply_event(
        &mut self,
        pending: &mut PendingEvent,
        budget: &mut SummaryBudget,
    ) -> bool {
        match pending {
            PendingEvent::Loaded {
                namespace,
                region,
                columns,
                after,
            } => {
                while let Some((pos, disk)) = columns.as_slice().first() {
                    let queued = (self.namespace == Some(*namespace))
                        .then(|| self.queued(*namespace, *pos))
                        .flatten();
                    let newest = queued
                        .filter(|queued| queued.version > disk.version)
                        .unwrap_or(disk);
                    if !budget.take(disk.summary.bytes.len().max(newest.summary.bytes.len())) {
                        return false;
                    }
                    let newest = newest.clone();
                    let (pos, _) = columns.next().expect("checked above");
                    if self.namespace == Some(*namespace) && newest.version > self.discarded_through
                    {
                        self.store.admit_versioned(pos, newest);
                    }
                }
                if self.namespace != Some(*namespace) {
                    return true;
                }
                loop {
                    let bounds = (after.map_or(Unbounded, Excluded), Unbounded);
                    let key = (*namespace, *region);
                    let Some((&pos, newest)) = self
                        .pending
                        .get(&key)
                        .and_then(|pending| pending.columns.range(bounds).next())
                    else {
                        return true;
                    };
                    if !budget.take(newest.summary.bytes.len()) {
                        return false;
                    }
                    let newest = newest.clone();
                    if newest.version > self.discarded_through {
                        self.store.admit_versioned(pos, newest);
                    }
                    *after = Some(pos);
                }
            }
            PendingEvent::Written {
                namespace,
                region,
                columns,
            } => {
                let key = (*namespace, *region);
                while let Some((pos, _)) = columns.as_slice().first() {
                    let bytes = self
                        .pending
                        .get(&key)
                        .and_then(|pending| pending.columns.get(pos))
                        .map_or(0, |column| column.summary.bytes.len());
                    if !budget.take(bytes) {
                        return false;
                    }
                    let (pos, version) = columns.next().expect("checked above");
                    if let Some(pending) = self.pending.get_mut(&key) {
                        let writing = &mut pending.columns;
                        if writing
                            .get(&pos)
                            .is_some_and(|column| column.version <= version)
                        {
                            writing.remove(&pos);
                        }
                    }
                }
                if let Some(pending) = self.pending.get_mut(&key) {
                    pending.in_flight = false;
                    if pending.columns.is_empty() {
                        self.pending.remove(&key);
                    }
                }
                true
            }
        }
    }
}

#[cfg(test)]
mod tests;
