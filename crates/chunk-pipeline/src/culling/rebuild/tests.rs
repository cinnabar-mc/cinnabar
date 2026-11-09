use std::collections::HashSet;

use super::*;
use crate::culling::{grid::Slot, tests::grid};
use meshing::FaceConnectivity;

/// A section on the horizontal test corridor.
fn key(x: i32) -> SubChunkKey {
    SubChunkKey::new(0, x, 0, 0)
}

/// A corridor `0..length` plus a dead end at -1 whose removal leaves a superset.
fn corridor(length: i32) -> ConnectivityGrid {
    grid((-1..length).map(|x| (key(x), FaceConnectivity::all())))
}

/// Holds the worker's reply until the test has made its intervening world changes.
struct Controlled {
    rebuild: CaveVisibilityRebuild,
    requests: Receiver<Job>,
    replies: SyncSender<Completed>,
    scratch: CaveVisibilityScratch,
    visible: CaveVisibleSet,
    replacement: CaveVisibleSet,
    /// Sections the app shows, flipped only as its publication would flip them.
    shown: HashSet<SubChunkKey>,
    initialized: bool,
}

impl Controlled {
    /// Installs bounded channels without spawning a thread. A zero inline budget sends every
    /// due rebuild to the controlled worker.
    fn new() -> Self {
        let (jobs, requests) = sync_channel(CHANNEL_CAPACITY);
        let (replies, done) = sync_channel(CHANNEL_CAPACITY);
        Self {
            rebuild: CaveVisibilityRebuild {
                worker: Some(Worker {
                    jobs,
                    done: Mutex::new(done),
                }),
                cells_per_inline_exit: usize::MAX,
                ..Default::default()
            },
            requests,
            replies,
            scratch: CaveVisibilityScratch::default(),
            visible: CaveVisibleSet::default(),
            replacement: CaveVisibleSet::default(),
            shown: HashSet::new(),
            initialized: false,
        }
    }

    /// Publishes one nonblocking update the way the app does: additions only, or a diff of the
    /// replacement against the previously published set.
    fn step(&mut self, camera: SubChunkKey, graph: &ConnectivityGrid) -> bool {
        let rebuilt = self.rebuild.update(
            camera,
            graph,
            &mut self.scratch,
            &mut self.visible,
            &mut self.replacement,
        );
        if !rebuilt {
            self.shown
                .extend(self.scratch.added_visible().iter().copied());
            return false;
        }
        if self.initialized {
            for key in self.visible.iter() {
                if !self.replacement.contains(&key) {
                    self.shown.remove(&key);
                }
            }
            for key in self.replacement.iter() {
                if !self.visible.contains(&key) {
                    self.shown.insert(key);
                }
            }
        } else {
            self.shown = self.replacement.iter().collect();
            self.initialized = true;
        }
        std::mem::swap(&mut self.visible, &mut self.replacement);
        true
    }

    /// Removes the dead end and updates until the superset is due, leaving its rebuild queued.
    fn queue_rebuild(&mut self, camera: SubChunkKey, graph: &mut ConnectivityGrid) {
        assert!(self.step(camera, graph), "the first update searches inline");
        graph.remove(&key(-1));
        let started = self.rebuild.stats().started;
        for _ in 0..SUPERSET_UPDATE_LIMIT {
            assert!(!self.step(camera, graph));
        }
        assert_eq!(self.rebuild.stats().started, started + 1);
        assert!(self.rebuild.pending);
    }

    /// Computes exactly the captured snapshot, not the subsequently edited live graph.
    fn finish(&self) {
        let job = self
            .requests
            .try_recv()
            .expect("a full search was requested");
        assert!(self.replies.send(job.run()).is_ok());
    }

    /// Whether the app would draw `section`.
    fn allows(&self, section: SubChunkKey) -> bool {
        self.shown.contains(&section)
    }

    /// Checks final visibility against the synchronous traversal of the current graph.
    fn assert_exact(&self, camera: SubChunkKey, graph: &ConnectivityGrid) {
        let mut scratch = CaveVisibilityScratch::default();
        scratch.set_outward_only(self.scratch.outward_only);
        let mut expected = CaveVisibleSet::default();
        fill_visible(camera, graph, &mut scratch, &mut expected);
        assert!(self.scratch.is_exact());
        assert_eq!(self.visible, expected);
        assert_eq!(self.shown, expected.iter().collect::<HashSet<_>>());
    }
}

#[test]
fn a_due_superset_rebuild_runs_on_the_worker_while_the_superset_stays_drawn() {
    let mut graph = corridor(5);
    let mut test = Controlled::new();
    test.queue_rebuild(key(0), &mut graph);
    assert!(
        !test.scratch.work().rebuilt,
        "no full search ran on the caller"
    );
    assert!((0..5).all(|x| test.allows(key(x))));
    test.finish();
    assert!(test.step(key(0), &graph));
    assert_eq!(test.scratch.work().explored_exits, 0);
    test.assert_exact(key(0), &graph);
}

#[test]
fn an_invalidating_move_after_a_large_search_publishes_without_showing_everything() {
    let mut sections: Vec<_> = (0..30)
        .flat_map(|x| (0..30).map(move |z| (SubChunkKey::new(0, x, 0, z), FaceConnectivity::all())))
        .collect();
    let small = SubChunkKey::new(0, 0, 10, 0);
    sections.push((small, FaceConnectivity::all()));
    let graph = grid(sections);
    let mut test = Controlled::new();
    let large = SubChunkKey::new(0, 0, 0, 0);
    assert!(test.step(large, &graph));
    assert!(test.scratch.work().explored_exits > graph.cell_count() / CELLS_PER_INLINE_EXIT);
    assert!(test.step(small, &graph), "published in the same update");
    assert!(!test.allows(large), "the move must not show every section");
    assert_eq!(test.rebuild.stats().started, 0);
    test.assert_exact(small, &graph);
}

#[test]
fn an_opening_is_applied_inline_while_a_safe_superset_is_being_rebuilt() {
    let mut graph = corridor(4);
    graph.insert(key(1), FaceConnectivity::none());
    let mut test = Controlled::new();
    test.queue_rebuild(key(0), &mut graph);
    assert!(!test.allows(key(3)));
    graph.insert(key(1), FaceConnectivity::all());
    assert!(!test.step(key(0), &graph));
    assert!(
        test.allows(key(3)),
        "the opening is shown before the rebuild lands"
    );
    test.finish();
    assert!(test.step(key(0), &graph));
    test.assert_exact(key(0), &graph);
}

#[test]
fn sections_reached_by_the_update_that_forces_a_rebuild_are_shown() {
    // A removal leaves a superset; the update that reaches the rebuild limit also opens a wall.
    let mut graph = corridor(4);
    graph.insert(key(1), FaceConnectivity::none());
    let mut test = Controlled::new();
    test.rebuild = CaveVisibilityRebuild::synchronous();
    assert!(test.step(key(0), &graph));
    assert!(!test.allows(key(3)));
    graph.remove(&key(-1));
    for _ in 1..SUPERSET_UPDATE_LIMIT {
        assert!(!test.step(key(0), &graph));
    }
    graph.insert(key(1), FaceConnectivity::all());
    test.step(key(0), &graph);
    test.step(key(0), &graph);
    assert!(
        test.allows(key(3)),
        "a section the opening reached must be flipped visible"
    );
    test.assert_exact(key(0), &graph);
}

#[test]
fn a_cheap_due_rebuild_runs_inline_without_a_snapshot() {
    let mut graph = corridor(3);
    let mut test = Controlled::new();
    test.rebuild.cells_per_inline_exit = CELLS_PER_INLINE_EXIT;
    assert!(test.step(key(0), &graph));
    graph.remove(&key(-1));
    for _ in 1..SUPERSET_UPDATE_LIMIT {
        assert!(!test.step(key(0), &graph));
    }
    assert!(
        test.step(key(0), &graph),
        "the limit update rebuilds in place"
    );
    assert_eq!(test.rebuild.stats().started, 0);
    assert!(test.requests.try_recv().is_err(), "no snapshot was taken");
    test.assert_exact(key(0), &graph);
}

#[test]
fn a_reused_slot_does_not_inherit_the_explored_exits_of_its_old_occupant() {
    // A row at z = 1 reaches x = 33, whose z = 0 neighbour reuses the cell of key(1) on the
    // toroidal grid. The beyond section at x = 34 is reachable only through that reused cell.
    let row = (0..=33).map(|x| (SubChunkKey::new(0, x, 0, 1), FaceConnectivity::all()));
    let mut graph = grid(
        [key(0), key(1), key(34)]
            .map(|key| (key, FaceConnectivity::all()))
            .into_iter()
            .chain(row),
    );
    let mut test = Controlled::new();
    assert!(test.step(key(0), &graph));
    assert!(!test.allows(key(34)));
    let slot = graph.slot(key(1));
    graph.remove(&key(1));
    graph.insert(key(33), FaceConnectivity::all());
    assert!(matches!((slot, graph.slot(key(33))), (Slot::Cell(a, _), Slot::Cell(b, _)) if a == b));
    assert!(!test.step(key(0), &graph));
    assert!(test.allows(key(33)) && test.allows(key(34)));
    for _ in 0..SUPERSET_UPDATE_LIMIT {
        test.step(key(0), &graph);
    }
    test.finish();
    assert!(test.step(key(0), &graph));
    test.assert_exact(key(0), &graph);
}

#[test]
fn a_camera_leaving_the_graph_while_a_rebuild_is_pending_searches_inline() {
    let mut graph = corridor(5);
    let mut test = Controlled::new();
    test.queue_rebuild(key(0), &mut graph);
    let outside = key(100);
    assert!(test.step(outside, &graph));
    test.assert_exact(outside, &graph);
    test.finish();
    assert!(!test.step(outside, &graph));
    assert_eq!(test.rebuild.stats().discarded, 1);
    test.assert_exact(outside, &graph);
}

#[test]
fn a_relayout_while_a_rebuild_is_pending_discards_the_snapshot() {
    let mut graph = corridor(5);
    let mut test = Controlled::new();
    test.queue_rebuild(key(0), &mut graph);
    let dims = graph.dims();
    graph.reserve_view(64, 32);
    assert_ne!(graph.dims(), dims);
    assert!(test.step(key(0), &graph));
    test.finish();
    assert!(!test.step(key(0), &graph));
    assert_eq!(test.rebuild.stats().discarded, 1);
    test.assert_exact(key(0), &graph);
}

#[test]
fn a_change_log_overflow_while_a_rebuild_is_pending_discards_the_snapshot() {
    let mut graph = corridor(5);
    let mut test = Controlled::new();
    test.queue_rebuild(key(0), &mut graph);
    for _ in 0..2100 {
        graph.insert(key(50), FaceConnectivity::all());
        graph.remove(&key(50));
    }
    assert!(test.step(key(0), &graph));
    test.finish();
    assert!(!test.step(key(0), &graph));
    assert_eq!(test.rebuild.stats().discarded, 1);
    test.assert_exact(key(0), &graph);
}

#[test]
fn returning_to_identical_matrices_in_a_new_grid_still_rejects_the_old_generation() {
    let mut original = corridor(5);
    let mut test = Controlled::new();
    test.queue_rebuild(key(0), &mut original);
    let current = corridor(5);
    assert!(test.step(key(0), &current));
    test.finish();
    assert!(!test.step(key(0), &current));
    assert_eq!(test.rebuild.stats().discarded, 1);
    test.assert_exact(key(0), &current);
}

#[test]
fn a_rule_change_while_a_rebuild_is_pending_discards_the_snapshot() {
    let mut graph = corridor(5);
    let mut test = Controlled::new();
    test.queue_rebuild(key(0), &mut graph);
    test.scratch.set_outward_only(true);
    assert!(test.step(key(0), &graph));
    test.finish();
    assert!(!test.step(key(0), &graph));
    assert_eq!(test.rebuild.stats().discarded, 1);
    test.assert_exact(key(0), &graph);
}

#[test]
fn a_failed_worker_leaves_due_rebuilds_on_the_caller() {
    let mut graph = corridor(5);
    let mut test = Controlled::new();
    test.queue_rebuild(key(0), &mut graph);
    let (disconnected, _) = sync_channel(CHANNEL_CAPACITY);
    test.replies = disconnected;
    assert!(test.step(key(0), &graph), "the due rebuild runs inline");
    assert!(test.rebuild.stats().failed);
    test.assert_exact(key(0), &graph);
}

#[test]
fn a_failed_thread_start_leaves_due_rebuilds_on_the_caller() {
    let mut graph = corridor(5);
    let mut test = Controlled::new();
    test.rebuild = CaveVisibilityRebuild {
        spawn: || None,
        cells_per_inline_exit: usize::MAX,
        ..Default::default()
    };
    assert!(test.step(key(0), &graph));
    graph.remove(&key(-1));
    for _ in 0..SUPERSET_UPDATE_LIMIT {
        assert!(!test.step(key(0), &graph));
    }
    assert!(test.step(key(0), &graph), "the next update rebuilds inline");
    assert_eq!(test.rebuild.stats().started, 0);
    test.assert_exact(key(0), &graph);
}

#[test]
fn the_real_worker_returns_the_same_visibility_as_synchronous_traversal() {
    let mut graph = grid((-64..64).map(|x| (key(x), FaceConnectivity::all())));
    let (mut rebuild, mut scratch, mut visible, mut replacement) = (
        CaveVisibilityRebuild {
            cells_per_inline_exit: usize::MAX,
            ..Default::default()
        },
        CaveVisibilityScratch::default(),
        CaveVisibleSet::default(),
        CaveVisibleSet::default(),
    );
    let mut update = |graph: &ConnectivityGrid| {
        let rebuilt = rebuild.update(key(0), graph, &mut scratch, &mut visible, &mut replacement);
        if rebuilt {
            std::mem::swap(&mut visible, &mut replacement);
        }
        rebuilt
    };
    assert!(update(&graph));
    graph.remove(&key(-64));
    for _ in 0..SUPERSET_UPDATE_LIMIT {
        assert!(!update(&graph));
    }
    test_time::eventually("a current cave traversal", || update(&graph));
    let mut expected = CaveVisibleSet::default();
    fill_visible(
        key(0),
        &graph,
        &mut CaveVisibilityScratch::default(),
        &mut expected,
    );
    assert_eq!(visible, expected);
    assert_eq!(rebuild.stats().started, 1);
    assert_eq!(rebuild.stats().completed, 1);
}
