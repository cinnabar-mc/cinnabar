//! Full connectivity searches over immutable, revision-tagged worker snapshots.

use std::sync::{
    Mutex,
    mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
};

use super::{
    ABANDONED, CaveVisibilityScratch, CaveVisibilityWork, CaveVisibleSet, ConnectivityGrid,
    SubChunkKey, fill_visible, fill_visible_within,
    incremental::{RetainedUpdate, update_incrementally},
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
    /// Searches handed to the worker as a grid snapshot.
    pub started: u64,
    pub completed: u64,
    /// Searches that finished on the caller within the snapshot-cost budget.
    pub inline: u64,
    pub discarded: u64,
    pub failed: bool,
}

/// A search may run on the caller for one explored exit per this many grid cells: exploring
/// an exit costs roughly what copying eight cells into a worker snapshot does.
const CELLS_PER_INLINE_EXIT: usize = 8;

/// At most one full search. A search expected to cost less than copying the grid runs on the
/// caller under a budget of that cost; a larger one runs on a worker over an immutable snapshot,
/// so the caller never pays more than about one snapshot copy. Incremental graph changes remain
/// inline. A failed worker leaves culling disabled; where no worker thread can start, every
/// search runs on the caller.
pub struct CaveVisibilityRebuild {
    worker: Option<Worker>,
    spawn: fn() -> Option<Worker>,
    pending: bool,
    synchronous: bool,
    cells_per_inline_exit: usize,
    /// A search that finished on the caller, published through the same catch-up as a reply.
    ready: Option<Completed>,
    /// Exits the latest full search explored, which predicts whether the next fits inline.
    last_exits: Option<usize>,
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
            ready: None,
            last_exits: None,
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

    /// Publishes only a completed search that can catch up to the current grid and camera.
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
        if let Some(update) = self.publish(camera, grid, scratch, replacement) {
            return update;
        }
        if self.stats.failed {
            return CaveVisibilityUpdate::Unavailable;
        }
        let retained = update_incrementally(camera, grid, scratch, visible);
        if retained == RetainedUpdate::Updated {
            return CaveVisibilityUpdate::Incremental;
        }
        if !self.pending {
            self.search(camera, grid, scratch.outward_only);
        }
        if retained == RetainedUpdate::Rebuild {
            // `visible` already holds this update's additions, which only an incremental
            // publication shows; a finished search is published on the next update.
            return CaveVisibilityUpdate::Incremental;
        }
        // The failed proof may already have propagated changes it never published, so only a
        // completed search may hide sections again.
        scratch.checkpoint = ABANDONED;
        // `visible` is unchanged, so a search that finished inline replaces it at once.
        self.publish(camera, grid, scratch, replacement)
            .unwrap_or(CaveVisibilityUpdate::Unavailable)
    }

    /// Swaps in a finished search once it catches up with the current graph and camera.
    fn publish(
        &mut self,
        camera: SubChunkKey,
        grid: &ConnectivityGrid,
        scratch: &mut CaveVisibilityScratch,
        replacement: &mut CaveVisibleSet,
    ) -> Option<CaveVisibilityUpdate> {
        let mut done = match self.ready.take() {
            Some(done) => done,
            None if self.pending => {
                let reply = self
                    .worker
                    .as_mut()
                    .expect("a pending cave search owns a worker")
                    .done
                    .get_mut()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .try_recv();
                match reply {
                    Err(TryRecvError::Empty) => return None,
                    Err(TryRecvError::Disconnected) => {
                        self.fail();
                        return Some(CaveVisibilityUpdate::Unavailable);
                    }
                    Ok(done) => {
                        self.pending = false;
                        self.stats.completed += 1;
                        done
                    }
                }
            }
            None => return None,
        };
        let searched = done.scratch.work.explored_exits;
        self.last_exits = Some(searched);
        done.scratch.outward_only = scratch.outward_only;
        if update_incrementally(camera, grid, &mut done.scratch, &mut done.visible)
            == RetainedUpdate::Invalid
        {
            self.stats.discarded += 1;
            self.spare = (done.scratch, done.visible);
            return None;
        }
        std::mem::swap(scratch, &mut done.scratch);
        std::mem::swap(replacement, &mut done.visible);
        self.spare = (done.scratch, done.visible);
        // Counters describe caller work: catch-up, plus the search when it ran here.
        scratch.work.rebuilt = true;
        if done.inline {
            scratch.work.explored_exits += searched;
        }
        Some(CaveVisibilityUpdate::Rebuilt)
    }

    /// Searches inline within the snapshot-cost budget, or hands a snapshot to the worker.
    fn search(&mut self, camera: SubChunkKey, grid: &ConnectivityGrid, outward_only: bool) {
        let (mut scratch, mut visible) = std::mem::take(&mut self.spare);
        scratch.outward_only = outward_only;
        let budget = if self.synchronous {
            usize::MAX
        } else {
            grid.cell_count() / self.cells_per_inline_exit
        };
        let finished = self.last_exits.is_none_or(|exits| exits <= budget)
            && fill_visible_within(camera, grid, &mut scratch, &mut visible, budget);
        if !finished && self.worker.is_none() {
            self.worker = (self.spawn)();
            if self.worker.is_none() {
                self.synchronous = true;
                fill_visible(camera, grid, &mut scratch, &mut visible);
            }
        }
        if finished || self.synchronous {
            self.stats.inline += 1;
            self.ready = Some(Completed {
                scratch,
                visible,
                inline: true,
            });
            return;
        }
        let job = Job {
            // Clones preserve revision identity only for this immutable snapshot. New live
            // grids still receive independent identities through ConnectivityGrid::default.
            grid: grid.clone(),
            camera,
            scratch,
            visible,
        };
        let worker = self.worker.as_ref().expect("a worker was started above");
        if worker.jobs.try_send(job).is_err() {
            self.fail();
        } else {
            self.pending = true;
            self.stats.started += 1;
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
            inline: false,
        }
    }
}

/// The scratch contains the searched grid's identity, layout epoch and journal revision.
struct Completed {
    scratch: CaveVisibilityScratch,
    visible: CaveVisibleSet,
    inline: bool,
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
