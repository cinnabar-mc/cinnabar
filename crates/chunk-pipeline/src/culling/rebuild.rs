//! Full connectivity searches: inline whenever the current proof is invalid, and on a worker
//! over an immutable, revision-tagged snapshot when only a still-safe superset is due a rebuild.

use std::sync::{
    Mutex,
    mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
};

use super::{
    CaveVisibilityScratch, CaveVisibilityWork, CaveVisibleSet, ConnectivityGrid, SubChunkKey,
    fill_visible,
    incremental::{RetainedUpdate, SUPERSET_UPDATE_LIMIT, update_incrementally},
};

#[cfg(test)]
mod tests;

/// Each direction buffers one message; the owner permits only one outstanding search.
const CHANNEL_CAPACITY: usize = 1;

/// A search explores an exit in roughly the time a snapshot copies this many grid cells.
const CELLS_PER_INLINE_EXIT: usize = 8;

/// Full-search receipts, independent of the frame on which a result is published.
#[derive(Clone, Copy, Debug, Default)]
pub struct CaveRebuildStats {
    /// Superset rebuilds handed to the worker as a grid snapshot.
    pub started: u64,
    pub completed: u64,
    /// Full searches run on the caller.
    pub inline: u64,
    pub discarded: u64,
    pub failed: bool,
}

/// Schedules full searches. An invalid proof is replaced at once by an inline search, so
/// nothing is ever drawn on a stale result. A still-safe superset that is due a rebuild keeps
/// being drawn while a worker searches a snapshot, unless the retained reach shows the search
/// would cost less than the snapshot copy. Where no worker can run, rebuilds stay inline.
pub struct CaveVisibilityRebuild {
    worker: Option<Worker>,
    spawn: fn() -> Option<Worker>,
    pending: bool,
    synchronous: bool,
    cells_per_inline_exit: usize,
    spare: (CaveVisibilityScratch, CaveVisibleSet),
    stats: CaveRebuildStats,
}

impl Default for CaveVisibilityRebuild {
    fn default() -> Self {
        Self {
            worker: None,
            spawn: Worker::spawn,
            pending: false,
            synchronous: false,
            cells_per_inline_exit: CELLS_PER_INLINE_EXIT,
            spare: Default::default(),
            stats: CaveRebuildStats::default(),
        }
    }
}

impl CaveVisibilityRebuild {
    /// Runs every full search on the caller, as where no worker thread can start.
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

    /// Extends `visible`, or fills `replacement` and returns true. A worker result is published
    /// only after it catches up with the current grid and camera through the same incremental
    /// proof as inline work, which checks grid identity, layout epoch and journal revision.
    pub(crate) fn update(
        &mut self,
        camera: SubChunkKey,
        grid: &ConnectivityGrid,
        scratch: &mut CaveVisibilityScratch,
        visible: &mut CaveVisibleSet,
        replacement: &mut CaveVisibleSet,
    ) -> bool {
        scratch.work = CaveVisibilityWork::default();
        if self.publish(camera, grid, scratch, replacement) {
            return true;
        }
        let rebuild_due = scratch
            .superset_updates
            .is_some_and(|updates| updates.saturating_add(1) >= SUPERSET_UPDATE_LIMIT);
        if rebuild_due && !self.pending && self.rebuilds_inline(grid, scratch) {
            // Search before applying changes, so `visible` still holds the published set.
            self.search_inline(camera, grid, scratch, replacement);
            return true;
        }
        match update_incrementally(camera, grid, scratch, visible) {
            RetainedUpdate::Updated => false,
            RetainedUpdate::Invalid => {
                self.search_inline(camera, grid, scratch, replacement);
                true
            }
            RetainedUpdate::Rebuild => {
                // `visible` stays a safe superset holding this update's additions.
                if !self.pending {
                    self.dispatch(camera, grid, scratch.outward_only);
                }
                false
            }
        }
    }

    /// A rebuild explores at most six exits per node the retained superset reached, so it
    /// runs inline when that bound costs no more than copying the grid.
    fn rebuilds_inline(&self, grid: &ConnectivityGrid, scratch: &CaveVisibilityScratch) -> bool {
        self.synchronous
            || scratch.touched.len().saturating_mul(6)
                <= grid.cell_count() / self.cells_per_inline_exit
    }

    /// Fills `replacement` on the caller; the failed incremental attempt stays in the counters.
    fn search_inline(
        &mut self,
        camera: SubChunkKey,
        grid: &ConnectivityGrid,
        scratch: &mut CaveVisibilityScratch,
        replacement: &mut CaveVisibleSet,
    ) {
        let attempted = scratch.work;
        fill_visible(camera, grid, scratch, replacement);
        scratch.work.explored_exits += attempted.explored_exits;
        scratch.work.proof_exits = attempted.proof_exits;
        scratch.work.additions = attempted.additions;
        self.stats.inline += 1;
    }

    /// Swaps in a worker result once it catches up with the current graph and camera.
    fn publish(
        &mut self,
        camera: SubChunkKey,
        grid: &ConnectivityGrid,
        scratch: &mut CaveVisibilityScratch,
        replacement: &mut CaveVisibleSet,
    ) -> bool {
        if !self.pending {
            return false;
        }
        let Some(worker) = self.worker.as_mut() else {
            return false;
        };
        let reply = worker
            .done
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .try_recv();
        let mut done = match reply {
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => {
                self.fail();
                return false;
            }
            Ok(done) => done,
        };
        self.pending = false;
        self.stats.completed += 1;
        done.scratch.outward_only = scratch.outward_only;
        if update_incrementally(camera, grid, &mut done.scratch, &mut done.visible)
            == RetainedUpdate::Invalid
        {
            self.stats.discarded += 1;
            self.spare = (done.scratch, done.visible);
            return false;
        }
        std::mem::swap(scratch, &mut done.scratch);
        std::mem::swap(replacement, &mut done.visible);
        self.spare = (done.scratch, done.visible);
        // The counters now describe inline catch-up; the full search ran on the worker.
        scratch.work.rebuilt = true;
        true
    }

    /// Hands a snapshot to the worker; without one, the next due update rebuilds inline.
    fn dispatch(&mut self, camera: SubChunkKey, grid: &ConnectivityGrid, outward_only: bool) {
        if self.worker.is_none() {
            self.worker = (self.spawn)();
        }
        let Some(worker) = &self.worker else {
            self.synchronous = true;
            return;
        };
        let (mut scratch, visible) = std::mem::take(&mut self.spare);
        scratch.outward_only = outward_only;
        let job = Job {
            // Clones preserve revision identity only for this immutable snapshot. New live
            // grids still receive independent identities through ConnectivityGrid::default.
            grid: grid.clone(),
            camera,
            scratch,
            visible,
        };
        if worker.jobs.try_send(job).is_err() {
            self.fail();
        } else {
            self.pending = true;
            self.stats.started += 1;
        }
    }

    /// Drops failed channels without blocking on a thread; later rebuilds run inline.
    fn fail(&mut self) {
        self.worker = None;
        self.pending = false;
        self.synchronous = true;
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
    /// Starts a worker lazily; failure leaves rebuilds on the caller.
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
