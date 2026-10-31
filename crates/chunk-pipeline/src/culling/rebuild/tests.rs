use super::*;
use crate::culling::{incremental::SUPERSET_UPDATE_LIMIT, tests::grid};
use meshing::FaceConnectivity;

/// A section on the horizontal test corridor.
fn key(x: i32) -> SubChunkKey {
    SubChunkKey::new(0, x, 0, 0)
}

/// Holds the worker's reply until the test has made its intervening world changes.
struct Controlled {
    rebuild: CaveVisibilityRebuild,
    requests: Receiver<Job>,
    replies: SyncSender<Completed>,
    scratch: CaveVisibilityScratch,
    visible: CaveVisibleSet,
    replacement: CaveVisibleSet,
    last: CaveVisibilityUpdate,
}

impl Controlled {
    /// Installs bounded channels without spawning a thread or advancing a clock.
    fn new() -> Self {
        let (jobs, requests) = sync_channel(CHANNEL_CAPACITY);
        let (replies, done) = sync_channel(CHANNEL_CAPACITY);
        Self {
            rebuild: CaveVisibilityRebuild {
                worker: Some(Worker {
                    jobs,
                    done: Mutex::new(done),
                }),
                ..Default::default()
            },
            requests,
            replies,
            scratch: CaveVisibilityScratch::default(),
            visible: CaveVisibleSet::default(),
            replacement: CaveVisibleSet::default(),
            last: CaveVisibilityUpdate::Unavailable,
        }
    }

    /// Applies the public publication contract after one nonblocking update.
    fn step(&mut self, camera: SubChunkKey, graph: &ConnectivityGrid) -> CaveVisibilityUpdate {
        self.last = self.rebuild.update(
            camera,
            graph,
            &mut self.scratch,
            &mut self.visible,
            &mut self.replacement,
        );
        if self.last == CaveVisibilityUpdate::Rebuilt {
            std::mem::swap(&mut self.visible, &mut self.replacement);
        }
        self.last
    }

    /// Computes exactly the captured snapshot, not the subsequently edited live graph.
    fn finish(&self) {
        let job = self
            .requests
            .try_recv()
            .expect("a full search was requested");
        assert!(self.replies.send(job.run()).is_ok());
    }

    /// An unavailable proof cannot hide any section, including one outside the old reach.
    fn allows(&self, section: SubChunkKey) -> bool {
        self.last == CaveVisibilityUpdate::Unavailable || self.visible.contains(&section)
    }

    /// Checks final visibility against the synchronous traversal of the current graph.
    fn assert_exact(&self, camera: SubChunkKey, graph: &ConnectivityGrid) {
        let mut scratch = CaveVisibilityScratch::default();
        scratch.set_outward_only(self.scratch.outward_only);
        let mut expected = CaveVisibleSet::default();
        fill_visible(camera, graph, &mut scratch, &mut expected);
        assert!(self.scratch.is_exact());
        assert_eq!(self.visible, expected);
    }
}

#[test]
fn a_full_search_runs_only_when_the_worker_accepts_its_snapshot() {
    let graph = grid((0..5).map(|x| (key(x), FaceConnectivity::all())));
    let mut baseline = CaveVisibilityScratch::default();
    fill_visible(
        key(0),
        &graph,
        &mut baseline,
        &mut CaveVisibleSet::default(),
    );
    assert!(baseline.work().explored_exits > 0);

    let mut test = Controlled::new();
    assert_eq!(test.step(key(0), &graph), CaveVisibilityUpdate::Unavailable);
    assert_eq!(test.scratch.work(), CaveVisibilityWork::default());
    assert!(graph.keys().all(|section| test.allows(section)));
    assert_eq!(test.rebuild.stats().started, 1);
    test.finish();
    assert_eq!(test.step(key(0), &graph), CaveVisibilityUpdate::Rebuilt);
    assert_eq!(test.scratch.work().explored_exits, 0);
    test.assert_exact(key(0), &graph);
}

#[test]
fn an_opening_is_applied_inline_while_a_safe_superset_is_being_rebuilt() {
    let branch = SubChunkKey::new(0, 0, 0, 1);
    let mut graph = grid(
        (0..4)
            .map(|x| {
                (
                    key(x),
                    if x == 1 {
                        FaceConnectivity::none()
                    } else {
                        FaceConnectivity::all()
                    },
                )
            })
            .chain([
                (branch, FaceConnectivity::all()),
                (key(20), FaceConnectivity::none()),
            ]),
    );
    let mut test = Controlled::new();
    test.step(key(0), &graph);
    test.finish();
    test.step(key(0), &graph);
    assert!(!test.allows(key(3)));
    graph.insert(branch, FaceConnectivity::none());
    for _ in 0..SUPERSET_UPDATE_LIMIT {
        assert_eq!(test.step(key(0), &graph), CaveVisibilityUpdate::Incremental);
    }
    assert_eq!(test.rebuild.stats().started, 2);
    assert!(!test.allows(key(3)));
    assert!(!test.allows(key(20)));
    test.scratch.superset_updates = Some(u32::MAX);

    graph.insert(key(1), FaceConnectivity::all());
    assert_eq!(test.step(key(0), &graph), CaveVisibilityUpdate::Incremental);
    assert!(test.allows(key(3)));
    assert!(!test.allows(key(20)));
    assert_eq!(test.scratch.work().additions, 1);
    test.finish();
    assert_eq!(test.step(key(0), &graph), CaveVisibilityUpdate::Rebuilt);
    assert!(
        test.allows(key(3)),
        "an old result must not hide a new opening"
    );
    test.assert_exact(key(0), &graph);
}

#[test]
fn camera_escape_discards_the_old_result_and_restores_visibility_immediately() {
    let graph = grid([
        (key(0), FaceConnectivity::all()),
        (key(20), FaceConnectivity::all()),
    ]);
    let mut test = Controlled::new();
    test.step(key(0), &graph);
    assert_eq!(
        test.step(key(20), &graph),
        CaveVisibilityUpdate::Unavailable
    );
    assert!(test.allows(key(20)));
    test.finish();
    assert_eq!(
        test.step(key(20), &graph),
        CaveVisibilityUpdate::Unavailable
    );
    assert_eq!(test.rebuild.stats().discarded, 1);
    assert!(test.allows(key(20)));
    test.finish();
    assert_eq!(test.step(key(20), &graph), CaveVisibilityUpdate::Rebuilt);
    test.assert_exact(key(20), &graph);
}

#[test]
fn slot_reuse_does_not_inherit_the_old_occupants_reachability() {
    let mut graph = grid((0..3).map(|x| (key(x), FaceConnectivity::all())));
    let mut test = Controlled::new();
    test.step(key(0), &graph);
    let slot = graph.slot(key(1));
    graph.remove(&key(1));
    let reused = key(1 + (1 << graph.dims().0));
    graph.insert(reused, FaceConnectivity::all());
    assert!(
        matches!((slot, graph.slot(reused)), (super::super::grid::Slot::Cell(a, _), super::super::grid::Slot::Cell(b, _)) if a == b)
    );
    test.finish();
    assert_eq!(test.step(key(0), &graph), CaveVisibilityUpdate::Rebuilt);
    assert!(!test.allows(reused));
    for _ in 0..SUPERSET_UPDATE_LIMIT {
        test.step(key(0), &graph);
    }
    test.finish();
    assert_eq!(test.step(key(0), &graph), CaveVisibilityUpdate::Rebuilt);
    test.assert_exact(key(0), &graph);
}

#[test]
fn returning_to_identical_matrices_in_a_new_grid_still_rejects_the_old_generation() {
    let original = grid([(key(0), FaceConnectivity::all())]);
    let mut test = Controlled::new();
    test.step(key(0), &original);
    let intermediate = grid([(key(0), FaceConnectivity::none())]);
    test.step(key(0), &intermediate);
    let current = grid([(key(0), FaceConnectivity::all())]);
    test.finish();
    assert_eq!(
        test.step(key(0), &current),
        CaveVisibilityUpdate::Unavailable
    );
    assert_eq!(test.rebuild.stats().discarded, 1);
    test.finish();
    assert_eq!(test.step(key(0), &current), CaveVisibilityUpdate::Rebuilt);
    test.assert_exact(key(0), &current);
}

#[test]
fn a_rule_change_while_work_is_pending_requires_a_new_snapshot() {
    let graph = grid((-2..3).map(|x| (key(x), FaceConnectivity::all())));
    let mut test = Controlled::new();
    test.step(key(0), &graph);
    test.scratch.set_outward_only(true);
    test.finish();
    assert_eq!(test.step(key(0), &graph), CaveVisibilityUpdate::Unavailable);
    test.finish();
    assert_eq!(test.step(key(0), &graph), CaveVisibilityUpdate::Rebuilt);
    test.assert_exact(key(0), &graph);
}

#[test]
fn worker_failure_never_falls_back_to_a_full_search_on_the_caller() {
    let graph = grid([(key(0), FaceConnectivity::all())]);
    let mut test = Controlled::new();
    test.step(key(0), &graph);
    let (disconnected, _) = sync_channel(CHANNEL_CAPACITY);
    test.replies = disconnected;
    assert_eq!(test.step(key(0), &graph), CaveVisibilityUpdate::Unavailable);
    assert!(test.rebuild.stats().failed);
    assert!(test.allows(key(0)));
    assert_eq!(test.scratch.work(), CaveVisibilityWork::default());
    assert_eq!(test.step(key(0), &graph), CaveVisibilityUpdate::Unavailable);
    assert_eq!(test.rebuild.stats().started, 1);
}

#[test]
fn the_real_worker_returns_the_same_visibility_as_synchronous_traversal() {
    let graph = grid((-64..64).map(|x| (key(x), FaceConnectivity::all())));
    let (mut rebuild, mut scratch, mut visible, mut replacement) = (
        CaveVisibilityRebuild::default(),
        CaveVisibilityScratch::default(),
        CaveVisibleSet::default(),
        CaveVisibleSet::default(),
    );
    test_time::eventually("a current cave traversal", || {
        rebuild.update(key(0), &graph, &mut scratch, &mut visible, &mut replacement)
            == CaveVisibilityUpdate::Rebuilt
    });
    let mut expected = CaveVisibleSet::default();
    fill_visible(
        key(0),
        &graph,
        &mut CaveVisibilityScratch::default(),
        &mut expected,
    );
    assert_eq!(replacement, expected);
    assert_eq!(rebuild.stats().started, 1);
    assert_eq!(rebuild.stats().completed, 1);
}

#[test]
fn a_proof_invalidated_by_a_camera_move_stays_invalid_when_the_camera_returns() {
    // The departing update already propagated the opening, so reusing that proof would never
    // publish the sections it newly reached.
    let mut graph = grid([
        (key(0), FaceConnectivity::all()),
        (key(1), FaceConnectivity::none()),
        (key(2), FaceConnectivity::all()),
        (key(3), FaceConnectivity::all()),
        (key(20), FaceConnectivity::all()),
    ]);
    let mut test = Controlled::new();
    test.step(key(0), &graph);
    test.finish();
    assert_eq!(test.step(key(0), &graph), CaveVisibilityUpdate::Rebuilt);
    assert!(!test.allows(key(3)));
    graph.insert(key(1), FaceConnectivity::all());
    assert_eq!(
        test.step(key(20), &graph),
        CaveVisibilityUpdate::Unavailable
    );
    test.step(key(0), &graph);
    assert!(
        test.allows(key(3)),
        "the returning camera must not hide what the opening reached"
    );
    test.finish();
    test.step(key(0), &graph);
    test.finish();
    assert_eq!(test.step(key(0), &graph), CaveVisibilityUpdate::Rebuilt);
    test.assert_exact(key(0), &graph);
}

#[test]
fn without_a_worker_thread_full_searches_complete_on_the_caller() {
    let graph = grid([
        (key(0), FaceConnectivity::all()),
        (key(20), FaceConnectivity::all()),
    ]);
    let mut test = Controlled::new();
    test.rebuild = CaveVisibilityRebuild::synchronous();
    assert_eq!(test.step(key(0), &graph), CaveVisibilityUpdate::Rebuilt);
    test.assert_exact(key(0), &graph);
    assert_eq!(test.step(key(20), &graph), CaveVisibilityUpdate::Rebuilt);
    test.assert_exact(key(20), &graph);
    assert_eq!(test.rebuild.stats().started, 0);
}
