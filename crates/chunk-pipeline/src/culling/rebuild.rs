//! Full connectivity searches over immutable, revision-tagged worker snapshots.

use std::sync::{
    Mutex,
    mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
};

use super::{
    CaveVisibilityScratch, CaveVisibilityWork, CaveVisibleSet, ConnectivityGrid, SubChunkKey,
    fill_visible,
    incremental::{RetainedUpdate, rebuild_inline, update_incrementally, update_visible},
};

#[cfg(test)]
mod tests;

/// Each direction buffers one message; the owner permits only one outstanding search.
const CHANNEL_CAPACITY: usize = 1;

/// How the caller must publish this update's cave visibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaveVisibilityUpdate {
    /// `visible` remains conservative; publish only `scratch.added_visible()`.
    Incremental,
    /// `replacement` is current; swap it into `visible` before hiding sections.
    Rebuilt,
    /// No current proof is available; restore every hidden section immediately.
    Unavailable,
}

/// Full-search receipts, independent of the frame on which a result is published.
#[derive(Clone, Copy, Debug, Default)]
pub struct CaveRebuildStats {
    pub started: u64,
    pub completed: u64,
    pub discarded: u64,
    pub failed: bool,
}

/// At most one full search, with no waiting or full traversal on the caller's thread.
/// Incremental graph changes remain inline. A failed worker leaves culling disabled; where no
/// worker thread can start, full searches run on the caller instead.
#[derive(Default)]
pub struct CaveVisibilityRebuild {
    worker: Option<Worker>,
    pending: bool,
    synchronous: bool,
    spare: (CaveVisibilityScratch, CaveVisibleSet),
    stats: CaveRebuildStats,
}

impl CaveVisibilityRebuild {
    /// Runs every full search on the caller, as on targets without threads.
    pub fn synchronous() -> Self {
        Self {
            synchronous: true,
            ..Self::default()
        }
    }

    /// Returns scheduling counters without touching the worker's queues.
    pub fn stats(&self) -> CaveRebuildStats {
        self.stats
    }

    /// Publishes only a completed snapshot that can catch up to the current grid and camera.
    /// Grid identity, slot-layout epoch and journal revision are retained in the scratch;
    /// openings, slot reuse and camera escape must pass the same incremental proof as inline work.
    pub(crate) fn update(
        &mut self,
        camera: SubChunkKey,
        grid: &ConnectivityGrid,
        scratch: &mut CaveVisibilityScratch,
        visible: &mut CaveVisibleSet,
        replacement: &mut CaveVisibleSet,
    ) -> CaveVisibilityUpdate {
        scratch.work = CaveVisibilityWork::default();
        if self.synchronous {
            return if update_visible(camera, grid, scratch, visible, replacement) {
                CaveVisibilityUpdate::Rebuilt
            } else {
                CaveVisibilityUpdate::Incremental
            };
        }
        if self.pending {
            let result = self
                .worker
                .as_mut()
                .expect("a pending cave search owns a worker")
                .done
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .try_recv();
            match result {
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.fail();
                    return CaveVisibilityUpdate::Unavailable;
                }
                Ok(mut done) => {
                    self.pending = false;
                    self.stats.completed += 1;
                    done.scratch.outward_only = scratch.outward_only;
                    if update_incrementally(camera, grid, &mut done.scratch, &mut done.visible)
                        != RetainedUpdate::Invalid
                    {
                        std::mem::swap(scratch, &mut done.scratch);
                        std::mem::swap(replacement, &mut done.visible);
                        self.spare = (done.scratch, done.visible);
                        // The counters now describe inline catch-up; the full traversal ran
                        // on the worker rather than in this publication frame.
                        scratch.work.rebuilt = true;
                        return CaveVisibilityUpdate::Rebuilt;
                    }
                    self.stats.discarded += 1;
                    self.spare = (done.scratch, done.visible);
                }
            }
        }
        if self.stats.failed {
            return CaveVisibilityUpdate::Unavailable;
        }
        let retained = update_incrementally(camera, grid, scratch, visible);
        if retained == RetainedUpdate::Updated {
            return CaveVisibilityUpdate::Incremental;
        }
        let publication = if retained == RetainedUpdate::Invalid {
            // The failed proof may already have propagated changes it never published, so only
            // a completed search may hide sections again. Grid identities start at 1.
            scratch.checkpoint = (0, 0, 0);
            CaveVisibilityUpdate::Unavailable
        } else {
            CaveVisibilityUpdate::Incremental
        };
        if self.pending {
            return publication;
        }
        if self.worker.is_none() {
            self.worker = Worker::spawn();
            if self.worker.is_none() {
                self.synchronous = true;
                rebuild_inline(camera, grid, scratch, replacement);
                return CaveVisibilityUpdate::Rebuilt;
            }
        }
        let (mut job_scratch, job_visible) = std::mem::take(&mut self.spare);
        job_scratch.outward_only = scratch.outward_only;
        let job = Job {
            // Clones preserve revision identity only for this immutable snapshot. New live
            // grids still receive independent identities through ConnectivityGrid::default.
            grid: grid.clone(),
            camera,
            scratch: job_scratch,
            visible: job_visible,
        };
        if self.worker.as_ref().unwrap().jobs.try_send(job).is_err() {
            self.fail();
        } else {
            self.pending = true;
            self.stats.started += 1;
        }
        if self.stats.failed {
            CaveVisibilityUpdate::Unavailable
        } else {
            publication
        }
    }

    /// Drops failed channels without blocking on a thread or falling back to an inline search.
    fn fail(&mut self) {
        self.worker = None;
        self.pending = false;
        self.stats.failed = true;
    }
}

/// Owned matrices and reusable traversal storage for one full search.
struct Job {
    grid: ConnectivityGrid,
    camera: SubChunkKey,
    scratch: CaveVisibilityScratch,
    visible: CaveVisibleSet,
}

impl Job {
    /// Searches only the captured matrices; dropping them also happens on the worker.
    fn run(mut self) -> Completed {
        fill_visible(
            self.camera,
            &self.grid,
            &mut self.scratch,
            &mut self.visible,
        );
        Completed {
            scratch: self.scratch,
            visible: self.visible,
        }
    }
}

/// The scratch contains the snapshot's grid identity, layout epoch and journal revision.
struct Completed {
    scratch: CaveVisibilityScratch,
    visible: CaveVisibleSet,
}

/// One bounded request and reply; dropping the sender lets the worker exit normally.
struct Worker {
    jobs: SyncSender<Job>,
    /// Only the mutable owner reads this receiver. The mutex supplies Sync without locking.
    done: Mutex<Receiver<Completed>>,
}

impl Worker {
    /// Starts a worker lazily; thread or channel failure cannot authorize a hidden section.
    fn spawn() -> Option<Self> {
        let (jobs, requests) = sync_channel::<Job>(CHANNEL_CAPACITY);
        let (replies, done) = sync_channel(CHANNEL_CAPACITY);
        std::thread::Builder::new()
            .name("cave-visibility".into())
            .spawn(move || {
                while let Ok(job) = requests.recv() {
                    if replies.send(job.run()).is_err() {
                        break;
                    }
                }
            })
            .ok()?;
        Some(Self {
            jobs,
            done: Mutex::new(done),
        })
    }
}
