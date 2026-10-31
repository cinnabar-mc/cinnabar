use std::time::Duration;

use launcher::local_worlds::world::{Generator, UnavailableReason};

use super::{report::target_for, *};
use crate::local_worlds::{
    provision::{Platform, RuntimeInfo, RuntimeKind},
    test_support::{FakeInstance, FakeRunner, scratch},
};
use bridge::{LocalWorld, Transport};
use launcher::local_worlds::world::WorldStatus;

const MAC: Platform = Platform {
    os: "darwin",
    arch: "arm64",
};
const LINUX: Platform = Platform {
    os: "linux",
    arch: "amd64",
};

/// Shuts its manager down when dropped, as the Go tests' cleanup did.
struct Fixture {
    manager: Manager,
    _dir: tempfile::TempDir,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.manager.shutdown();
    }
}

fn fixture(build: impl FnOnce(&mut Parts, &tempfile::TempDir)) -> Fixture {
    let dir = scratch();
    let store = Arc::new(Store::open(dir.path().join("worlds")).expect("store"));
    let mut parts = Parts::new(store);
    let mut setup = Provisioner::new(dir.path().join("fixture-bds"), String::new());
    setup.platform = LINUX;
    let setup = Arc::new(setup);
    setup.accept_eula().expect("accept fixture EULA");
    parts.setup = Some(setup);
    build(&mut parts, &dir);
    Fixture {
        manager: Manager::from_parts(parts),
        _dir: dir,
    }
}

fn with_runner(runner: &Arc<FakeRunner>) -> Fixture {
    let runner: Arc<dyn Runner> = runner.clone();
    fixture(|parts, _| {
        parts.runner = Some(runner);
    })
}

fn flat(name: &str) -> NewWorld {
    NewWorld {
        name: name.to_owned(),
        generator: Generator::Flat,
        ..NewWorld::default()
    }
}

fn provisioner(dir: &tempfile::TempDir, platform: Platform) -> Arc<Provisioner> {
    let mut provisioner = Provisioner::new(dir.path().join("bds"), String::new());
    provisioner.platform = platform;
    Arc::new(provisioner)
}

#[track_caller]
fn wait_for(manager: &Manager, state: WorldState) -> WorldStatus {
    let mut status = manager.status();
    test_time::eventually(&format!("{state:?}"), || {
        status = manager.status();
        status.state == state
    });
    status
}

#[test]
fn open_runs_the_world_and_close_stops_its_server() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = with_runner(&runner);
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    manager.open(&world.id, None).expect("open");
    let status = wait_for(manager, WorldState::Running);
    assert_eq!(status.world_id.as_deref(), Some(world.id.as_str()));
    let report = manager.core_report();
    let target = report.target.expect("running target");
    assert_eq!(
        (target.address.as_str(), target.transport),
        ("127.0.0.1:1", Transport::NetherNet)
    );
    manager
        .open(&world.id, None)
        .expect("reopening the running world is a no-op");
    let instance = runner.last();
    manager.close();
    wait_for(manager, WorldState::Idle);
    assert!(instance.stopped(), "the server was not stopped");
    assert_eq!(manager.core_report(), LocalWorld::default());
}

/// A manager whose EULA is accepted, with one BDS world served by `runner`.
fn bds_world(runner: &Arc<FakeRunner>) -> (Fixture, World) {
    let runner: Arc<dyn Runner> = runner.clone();
    let fixture = fixture(|parts, dir| {
        parts.runner = Some(runner);
        parts.setup = Some(provisioner(dir, LINUX));
    });
    fixture.manager.accept_eula().expect("accept");
    let world = fixture
        .manager
        .create(&NewWorld {
            backend: Some(Backend::Bds),
            ..flat("w")
        })
        .expect("create");
    (fixture, world)
}

/// Saving a new default mode wakes every route subscriber without restarting the server.
#[test]
fn applied_game_mode_wakes_route_subscribers() {
    let runner = Arc::new(FakeRunner::default());
    let (fixture, world) = bds_world(&runner);
    let manager = &fixture.manager;
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    let before = manager.core_report();
    let subscribers = [manager.subscribe(), manager.subscribe()];
    manager
        .apply(
            &world.id,
            &WorldUpdate {
                game_mode: Some(bridge::GameMode::Creative),
                ..WorldUpdate::default()
            },
        )
        .expect("save the default mode");
    for changes in subscribers {
        changes
            .try_recv()
            .expect("wake for the changed world report");
    }
    manager.open(&world.id, None).expect("idempotent open");
    let after = manager.core_report();
    assert_eq!(after.state, WorldState::Running);
    assert_eq!(after.target, before.target);
    assert_eq!(
        after.world.expect("world").game_mode,
        bridge::GameMode::Creative
    );
    assert_eq!(lock(&runner.made).len(), 1, "the server stays running");
}

/// An in-game change reaches the running server at once and stays saved; a clean stop, after
/// which the server's own data holds it, ends it being pending.
#[test]
fn applied_settings_reach_the_running_server_and_persist() {
    use launcher::local_worlds::{
        in_world::{Action, update},
        options::WorldOption as O,
        world::Difficulty,
    };
    let runner = Arc::new(FakeRunner::default());
    let (fixture, world) = bds_world(&runner);
    let manager = &fixture.manager;
    let hard = update(&world, Action::Difficulty(Difficulty::Hard)).expect("an update");
    manager.apply(&world.id, &hard).expect("saved while closed");
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    let instance = runner.last();
    assert!(
        lock(&instance.console).is_empty(),
        "a closed world takes no commands"
    );
    let world = manager.inner.store.get(&world.id).expect("saved world");
    assert_eq!(world.difficulty, Difficulty::Hard);
    let cheats = update(&world, Action::Option(O::ActivateCheats, 1)).expect("cheats on");
    let world = manager.apply(&world.id, &cheats).expect("apply").world;
    let keep = update(&world, Action::Option(O::KeepInventory, 1)).expect("a rule");
    let applied = manager.apply(&world.id, &keep).expect("apply");
    assert!(!applied.missed);
    let saved = applied.world;
    assert!(saved.options.enabled(O::KeepInventory) && saved.options.enabled(O::ActivateCheats));
    assert_eq!(
        *lock(&instance.console),
        [
            "changesetting allow-cheats true",
            "gamerule keepinventory true"
        ]
    );
    let (_, pending, _) = manager.inner.store.snapshot(&world.id).expect("snapshot");
    assert_eq!(
        pending.get(O::KeepInventory),
        Some(1),
        "pending until the server saves"
    );
    manager.close();
    wait_for(manager, WorldState::Idle);
    let (reopened, pending, _) = manager.inner.store.snapshot(&world.id).expect("snapshot");
    assert!(pending.is_empty(), "{pending:?}");
    assert!(reopened.options.enabled(O::KeepInventory));
}

/// Applies a live PvP edit and checks its saved value before and after shutdown.
fn pending_after_live_edit(
    console_fails: bool,
    unclean: bool,
) -> launcher::local_worlds::options::OptionChanges {
    use launcher::local_worlds::{
        in_world::{Action, update},
        options::WorldOption as O,
    };
    let runner = Arc::new(FakeRunner {
        console_fails,
        unclean,
        ..FakeRunner::default()
    });
    let (fixture, world) = bds_world(&runner);
    let manager = &fixture.manager;
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    let world = manager.inner.store.get(&world.id).expect("saved world");
    let pvp = update(&world, Action::Option(O::Pvp, 0)).expect("an update");
    let applied = manager.apply(&world.id, &pvp).expect("saved");
    assert_eq!(applied.missed, console_fails);
    assert!(
        !applied.world.options.enabled(O::Pvp),
        "the change is saved either way"
    );
    manager.close();
    wait_for(manager, WorldState::Idle);
    let (saved, pending, _) = manager.inner.store.snapshot(&world.id).expect("snapshot");
    assert!(!saved.options.enabled(O::Pvp));
    pending
}

/// A failed console write is reported separately from persistence, even after a clean stop.
#[test]
fn a_failed_live_change_is_reported_and_stays_pending() {
    use launcher::local_worlds::options::WorldOption as O;
    assert_eq!(pending_after_live_edit(true, false).get(O::Pvp), Some(0));
}

/// A successful stop call without a clean exit must not acknowledge live changes.
#[test]
fn live_changes_stay_pending_after_an_unclean_stop() {
    use launcher::local_worlds::options::WorldOption as O;
    assert_eq!(pending_after_live_edit(false, true).get(O::Pvp), Some(0));
}

/// A crashed server's live edits cannot acknowledge a failed edit in its replacement.
#[test]
fn a_failed_live_change_after_reopening_a_crashed_server_stays_pending() {
    use launcher::local_worlds::{
        in_world::{Action, update},
        options::WorldOption as O,
    };
    for clear_failure in [false, true] {
        let runner = Arc::new(FakeRunner::default());
        let (fixture, world) = bds_world(&runner);
        let manager = &fixture.manager;
        manager.open(&world.id, None).expect("open");
        wait_for(manager, WorldState::Running);
        let pvp = update(&world, Action::Option(O::Pvp, 0)).expect("an update");
        assert!(!manager.apply(&world.id, &pvp).expect("applied").missed);
        runner.last().exit();
        wait_for(manager, WorldState::Failed);
        if clear_failure {
            manager.close();
        }
        manager.open(&world.id, None).expect("reopen");
        wait_for(manager, WorldState::Running);
        test_time::eventually("the replacement applies its startup settings", || {
            let (_, pending, _) = manager.inner.store.snapshot(&world.id).expect("snapshot");
            pending.is_empty()
        });
        runner
            .last()
            .console_fails
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(manager.apply(&world.id, &pvp).expect("saved").missed);
        manager.close();
        wait_for(manager, WorldState::Idle);
        let (saved, pending, _) = manager.inner.store.snapshot(&world.id).expect("snapshot");
        assert!(!saved.options.enabled(O::Pvp));
        assert_eq!(
            pending.get(O::Pvp),
            Some(0),
            "the failed edit must be retried"
        );
    }
}

/// Reopening a world that still saves starts its server only once the old one has exited.
#[test]
fn reopening_while_the_last_server_saves_waits_for_it() {
    let (runner, save) = FakeRunner::saving();
    let runner = Arc::new(runner);
    let fixture = with_runner(&runner);
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    let old = runner.last();
    manager.close();
    assert_eq!(manager.status().state, WorldState::Stopping);
    assert!(matches!(manager.delete(&world.id), Err(WorldError::InUse)));
    manager
        .open(&world.id, None)
        .expect("an open during the save is accepted");
    assert_eq!(manager.status().state, WorldState::Starting);
    drop(save);
    let status = wait_for(manager, WorldState::Running);
    assert_eq!(status.world_id.as_deref(), Some(world.id.as_str()));
    assert!(old.stopped());
    assert_eq!(lock(&runner.made).len(), 2);
    assert!(
        !runner.overlapped.load(std::sync::atomic::Ordering::SeqCst),
        "the new server started before the old one exited"
    );
}

/// A start cancelled by a close lets a following open wait for its server, not fail as busy.
#[test]
fn an_open_after_a_cancelled_start_runs_the_world() {
    let (runner, release) = FakeRunner::gated();
    let fixture = with_runner(&Arc::new(runner));
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    manager.open(&world.id, None).expect("open");
    manager.close();
    manager.open(&world.id, None).expect("reopen");
    drop(release);
    wait_for(manager, WorldState::Running);
}

/// Cancelling an open that waits on a saving world keeps that save's protection.
#[test]
fn cancelling_a_queued_open_keeps_the_earlier_save_pending() {
    let (runner, save) = FakeRunner::saving();
    let runner = Arc::new(runner);
    let fixture = with_runner(&runner);
    let manager = &fixture.manager;
    let saving = manager.create(&flat("a")).expect("create");
    let other = manager.create(&flat("b")).expect("create");
    manager.open(&saving.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    let old = runner.last();
    manager.close();
    manager
        .open(&other.id, None)
        .expect("an open queued behind the save");
    manager.close();
    wait_for(manager, WorldState::Idle);
    assert!(
        matches!(manager.delete(&saving.id), Err(WorldError::InUse)),
        "a world was deletable while its server still saved"
    );
    manager.open(&saving.id, None).expect("reopen");
    drop(save);
    wait_for(manager, WorldState::Running);
    assert!(old.stopped());
    assert!(
        !runner.overlapped.load(std::sync::atomic::Ordering::SeqCst),
        "a server started before the saving one exited"
    );
}

/// A reopen waits for a closed server's cleanup, which can outlast its process.
#[test]
fn a_reopen_waits_for_cleanup_after_the_old_server_exits() {
    let (runner, cleanup) = FakeRunner::cleaning();
    let runner = Arc::new(runner);
    let fixture = with_runner(&runner);
    let release_cleanup = cleanup;
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    let old = runner.last();
    manager.close();
    test_time::eventually("the old server to exit", || old.stopped());
    assert!(
        lock(&manager.inner.life)
            .retiring
            .iter()
            .any(|retiring| retiring.world_id == world.id && !retiring.done()),
        "the exit released the world before its cleanup finished"
    );
    manager.open(&world.id, None).expect("reopen");
    drop(release_cleanup);
    wait_for(manager, WorldState::Running);
    assert!(
        !runner.overlapped.load(std::sync::atomic::Ordering::SeqCst),
        "the new server started during the old one's cleanup"
    );
}

#[test]
fn another_world_is_busy_while_one_runs() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = with_runner(&runner);
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    let other = manager.create(&flat("other")).expect("create");
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    assert!(matches!(
        manager.open(&other.id, None),
        Err(WorldError::Busy)
    ));
    assert!(matches!(manager.delete(&world.id), Err(WorldError::InUse)));
    manager
        .delete(&other.id)
        .expect("another world can be deleted");
}

#[test]
fn opening_an_unknown_world_fails_and_stays_idle() {
    let fixture = with_runner(&Arc::new(FakeRunner::default()));
    let manager = &fixture.manager;
    assert!(matches!(
        manager.open("0123456789abcdef", None),
        Err(WorldError::NotFound)
    ));
    assert_eq!(manager.status().state, WorldState::Idle);
}

/// A failed start stays failed until closed and reports no local detail.
#[test]
fn a_failed_start_stays_failed_and_hides_detail() {
    let runner = Arc::new(FakeRunner {
        error: Some(|| WorldError::Failed("exec /secret/path: boom".into())),
        ..FakeRunner::default()
    });
    let fixture = with_runner(&runner);
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    manager.open(&world.id, None).expect("open");
    let status = wait_for(manager, WorldState::Failed);
    let error = status.error.expect("failure text");
    assert!(!error.is_empty() && !error.contains("secret"), "{error}");
    let report = manager.core_report();
    assert!(
        report.target.is_none() && report.error.is_some(),
        "a failed world must not fall through"
    );
    manager.close();
    wait_for(manager, WorldState::Idle);
}

#[test]
fn closing_while_starting_cancels_the_start() {
    let (runner, _release) = FakeRunner::gated();
    let fixture = with_runner(&Arc::new(runner));
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    manager.open(&world.id, None).expect("open");
    manager.close();
    wait_for(manager, WorldState::Idle);
}

/// The core holds a session while the world starts, then gets its target.
#[test]
fn the_core_report_follows_the_start() {
    let (runner, release) = FakeRunner::gated();
    let fixture = with_runner(&Arc::new(runner));
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    let changes = manager.subscribe();
    manager.open(&world.id, None).expect("open");
    changes
        .recv_timeout(test_time::DEFAULT_TIMEOUT)
        .expect("a change for the start");
    let starting = manager.core_report();
    assert_eq!(starting.state, WorldState::Starting);
    assert!(starting.target.is_none());
    assert_eq!(starting.world.expect("world").id, world.id);
    drop(release);
    wait_for(manager, WorldState::Running);
    assert!(manager.core_report().target.is_some());
}

#[test]
fn an_unexpected_exit_fails_the_world() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = with_runner(&runner);
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    runner.last().exit();
    wait_for(manager, WorldState::Failed);
}

#[test]
fn an_unexpected_exit_keeps_cleanup_ahead_of_reopen_and_delete() {
    for clear_failure in [false, true] {
        let (runner, cleanup) = FakeRunner::cleaning();
        let runner = Arc::new(runner);
        let fixture = with_runner(&runner);
        let release_cleanup = cleanup;
        let manager = &fixture.manager;
        let world = manager.create(&flat("w")).expect("create");
        manager.open(&world.id, None).expect("open");
        wait_for(manager, WorldState::Running);
        let old = runner.last();
        old.exit();
        wait_for(manager, WorldState::Failed);
        assert!(matches!(manager.delete(&world.id), Err(WorldError::InUse)));
        assert!(
            lock(&manager.inner.life)
                .retiring
                .iter()
                .any(|retiring| retiring.world_id == world.id && !retiring.done()),
            "failure must retain cleanup ownership before accepting a reopen"
        );
        if clear_failure {
            manager.close();
            assert_eq!(manager.status().state, WorldState::Idle);
        }
        manager.open(&world.id, None).expect("reopen");
        drop(release_cleanup);
        wait_for(manager, WorldState::Running);
        assert!(old.finished());
        assert!(
            !runner.overlapped.load(std::sync::atomic::Ordering::SeqCst),
            "the replacement started before the crashed server finished cleanup"
        );
    }
}

/// An exit that skips destructors still asks the running server to stop its own way.
#[test]
fn the_exit_sweep_asks_the_running_server_to_stop() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = with_runner(&runner);
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    manager.request_exit_stop();
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    manager.request_exit_stop();
    assert!(
        runner.last().stopped(),
        "the running server was not asked to stop"
    );
}

/// An exit while a world starts cancels the start, so it spawns nothing more.
#[test]
fn the_exit_sweep_cancels_a_start_in_progress() {
    let (runner, _release) = FakeRunner::gated();
    let runner = Arc::new(runner);
    let fixture = with_runner(&runner);
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    manager.open(&world.id, None).expect("open");
    manager.request_exit_stop();
    wait_for(manager, WorldState::Idle);
    assert!(
        lock(&runner.made).is_empty(),
        "the cancelled start made a server"
    );
}

/// BDS never reports simulation pause, including while its process starts.
#[test]
fn bds_status_never_claims_simulation_pause() {
    let (runner, release) = FakeRunner::gated();
    let runner = Arc::new(runner);
    let fixture = with_runner(&runner);
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    manager.open(&world.id, None).expect("open");
    let starting = manager.status();
    assert!(!starting.pause_supported && !starting.paused);
    drop(release);
    let running = wait_for(manager, WorldState::Running);
    assert!(!running.pause_supported && !running.paused);
}

#[test]
fn bds_worlds_need_a_supported_platform_and_the_eula() {
    let runner = Arc::new(FakeRunner::default());
    let unconfigured = fixture(|parts, _| parts.setup = None);
    let saved = unconfigured
        .manager
        .inner
        .store
        .create(&NewWorld {
            name: "b".into(),
            ..NewWorld::default()
        })
        .expect("save");
    assert_eq!(saved.backend, Backend::Bds);
    assert!(matches!(
        unconfigured.manager.open(&saved.id, None),
        Err(WorldError::BackendUnavailable)
    ));
    let configured = fixture(|parts, dir| {
        let runner: Arc<dyn Runner> = runner.clone();
        parts.runner = Some(Arc::clone(&runner));
        parts.setup = Some(provisioner(dir, LINUX));
    });
    let manager = &configured.manager;
    let world = manager
        .create(&NewWorld {
            name: "b".into(),
            backend: Some(Backend::Bds),
            ..NewWorld::default()
        })
        .expect("create");
    assert!(matches!(
        manager.open(&world.id, None),
        Err(WorldError::EulaRequired)
    ));
    let status = manager.status();
    assert_eq!(status.state, WorldState::Idle);
    assert_eq!(status.setup.expect("setup").state, SetupState::EulaRequired);
    manager.accept_eula().expect("accept");
    manager.open(&world.id, Some(8)).expect("open");
    let status = wait_for(manager, WorldState::Running);
    assert_eq!(status.backend, Some(Backend::Bds));
    assert_eq!(
        manager.core_report().target.expect("target").transport,
        Transport::NetherNet
    );
    let unsupported = fixture(|parts, dir| parts.setup = Some(provisioner(dir, MAC)));
    assert!(matches!(
        unsupported.manager.create(&NewWorld {
            name: "x".into(),
            backend: Some(Backend::Bds),
            ..NewWorld::default()
        }),
        Err(WorldError::BackendUnavailable)
    ));
    let dragonfly = NewWorld {
        backend: Some(Backend::LegacyDragonfly),
        ..flat("x")
    };
    unsupported
        .manager
        .create(&dragonfly)
        .expect_err("legacy hosts cannot create worlds");
}

#[test]
fn a_missing_runner_cannot_host_its_backend() {
    let fixture = fixture(|_, _| {});
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    manager.open(&world.id, None).expect("open");
    let status = wait_for(manager, WorldState::Failed);
    assert_eq!(
        status.error.as_deref(),
        Some(WorldError::BackendUnavailable.to_string().as_str())
    );
}

/// BDS advertising on LAN is reached through that advertisement and its selected level.
#[test]
fn bds_targets_use_lan_discovery_when_the_server_advertises() {
    let world = crate::local_worlds::test_support::world();
    let plain = FakeInstance::new();
    assert_eq!(target_for(&world, &plain).transport, Transport::NetherNet);
    let mut lan = FakeInstance::new();
    lan.lan = Some("127.0.0.1:7552".into());
    let target = target_for(&world, &lan);
    assert_eq!(
        (
            target.transport,
            target.lan_address.as_deref(),
            target.level_name.as_deref()
        ),
        (
            Transport::NetherNetLan,
            Some("127.0.0.1:7552"),
            Some(world.id.as_str())
        )
    );
    assert_eq!(target.address, "127.0.0.1:1");
}

/// A Retry that finds Docker enables creation without a fallback host.
#[test]
fn a_redetect_that_finds_docker_allows_bds_creation() {
    let fixture = fixture(|parts, dir| {
        let setup = provisioner(dir, MAC);
        setup.set_runtime(RuntimeInfo::none(
            "no docker",
            Some(UnavailableReason::DockerNotRunning),
        ));
        setup.set_detector(Arc::new(|| RuntimeInfo {
            kind: RuntimeKind::Container,
            reason: "docker up".into(),
            unavailable: None,
        }));
        parts.setup = Some(setup);
    });
    let manager = &fixture.manager;
    let status = manager.status();
    assert_eq!(
        status.backend_unavailable_reason,
        Some(UnavailableReason::DockerNotRunning)
    );
    assert_eq!(status.setup.expect("setup").state, SetupState::Unsupported);
    manager
        .prefs(PrefsUpdate {
            redetect: true,
            ..PrefsUpdate::default()
        })
        .expect("retry");
    let status = manager.status();
    assert_eq!(status.backend_unavailable_reason, None);
    assert_eq!(status.setup.expect("setup").runtime, "container");
    let world = manager
        .create(&NewWorld {
            name: "after".into(),
            ..NewWorld::default()
        })
        .expect("create");
    assert_eq!(world.backend, Backend::Bds);
}

#[test]
fn prefs_persist_through_the_manager() {
    let fixture = fixture(|_, _| {});
    let manager = &fixture.manager;
    assert!(
        !manager
            .prefs(PrefsUpdate::default())
            .expect("read")
            .docker_prompt_dismissed
    );
    let dismissed = PrefsUpdate {
        docker_prompt_dismissed: Some(true),
        redetect: false,
    };
    assert!(
        manager
            .prefs(dismissed)
            .expect("dismiss")
            .docker_prompt_dismissed
    );
    assert!(
        manager
            .prefs(PrefsUpdate::default())
            .expect("read")
            .docker_prompt_dismissed
    );
}

/// A macOS manager whose startup Docker probe is held until the test answers it.
fn pending(dir: &tempfile::TempDir, parts: &mut Parts) -> crossbeam_channel::Sender<RuntimeInfo> {
    let (answer, probe) = crossbeam_channel::bounded::<RuntimeInfo>(0);
    let setup = provisioner(dir, MAC);
    setup.set_detector(Arc::new(move || probe.recv().expect("probe answer")));
    parts.setup = Some(Arc::clone(&setup));
    setup.detect_in_background(RuntimeInfo::checking());
    answer
}

fn docker_down() -> RuntimeInfo {
    RuntimeInfo::none("down", Some(UnavailableReason::DockerNotRunning))
}

fn docker_up() -> RuntimeInfo {
    RuntimeInfo {
        kind: RuntimeKind::Container,
        reason: "docker up".into(),
        unavailable: None,
    }
}

/// Creation waits for detection and saves nothing when BDS cannot run.
#[test]
fn a_create_during_detection_waits_for_the_result() {
    let mut answer = None;
    let fixture = fixture(|parts, dir| answer = Some(pending(dir, parts)));
    let answer = answer.expect("probe");
    let manager = fixture.manager.clone();
    let (created, outcome) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = created.send(manager.create(&flat("flat")));
    });
    assert!(
        outcome.recv_timeout(Duration::from_millis(50)).is_err(),
        "the create returned before detection settled"
    );
    answer.send(docker_down()).expect("answer");
    let result = outcome
        .recv_timeout(test_time::DEFAULT_TIMEOUT)
        .expect("create returned");
    assert!(matches!(result, Err(WorldError::BackendUnavailable)));
    assert!(fixture.manager.list().expect("list").is_empty());
}

/// A probe that outlasts the wait refuses the create instead of saving a guess.
#[test]
fn a_create_refuses_while_detection_outlasts_the_wait() {
    let mut answer = None;
    let fixture = fixture(|parts, dir| {
        answer = Some(pending(dir, parts));
        parts.runtime_wait = Duration::from_millis(20);
    });
    let answer = answer.expect("probe");
    assert!(matches!(
        fixture.manager.create(&flat("flat")),
        Err(WorldError::RuntimePending)
    ));
    assert!(fixture.manager.list().expect("list").is_empty());
    answer.send(docker_up()).expect("answer");
}

/// A slow startup probe that lands after a newer Retry must not replace the Retry's result.
#[test]
fn an_overlapping_redetect_keeps_the_newest_result() {
    let dir = scratch();
    let (answers, probes) =
        crossbeam_channel::unbounded::<crossbeam_channel::Sender<RuntimeInfo>>();
    let setup = provisioner(&dir, MAC);
    setup.set_detector(Arc::new(move || {
        let (reply, answer) = crossbeam_channel::bounded(0);
        answers.send(reply).expect("probe started");
        answer.recv().expect("probe answer")
    }));
    let store = Arc::new(Store::open(dir.path().join("worlds")).expect("store"));
    let mut parts = Parts::new(store);
    parts.setup = Some(Arc::clone(&setup));
    let fixture = Fixture {
        manager: Manager::from_parts(parts),
        _dir: dir,
    };
    setup.detect_in_background(RuntimeInfo::checking());
    let startup = probes
        .recv_timeout(test_time::DEFAULT_TIMEOUT)
        .expect("startup probe");
    let manager = fixture.manager.clone();
    let retried = std::thread::spawn(move || {
        manager.prefs(PrefsUpdate {
            redetect: true,
            ..PrefsUpdate::default()
        })
    });
    probes
        .recv_timeout(test_time::DEFAULT_TIMEOUT)
        .expect("retry probe")
        .send(docker_up())
        .expect("answer the retry");
    retried.join().expect("retry thread").expect("retry");
    startup
        .send(docker_down())
        .expect("answer the startup probe");
    assert!(setup.await_runtime(test_time::DEFAULT_TIMEOUT));
    let status = fixture.manager.status();
    assert_eq!(status.setup.expect("setup").runtime, "container");
    assert_eq!(status.backend_unavailable_reason, None);
    let world = fixture
        .manager
        .create(&NewWorld {
            name: "normal".into(),
            ..NewWorld::default()
        })
        .expect("create");
    assert_eq!(world.backend, Backend::Bds);
}

#[test]
fn world_options_report_multiplayer_denial_to_the_core() {
    use launcher::local_worlds::options::WorldOption as O;
    let fixture = fixture(|_, _| {});
    let manager = &fixture.manager;
    {
        let mut life = lock(&manager.inner.life);
        let mut world = crate::local_worlds::test_support::world();
        world.options.set(O::Multiplayer, 0);
        life.world = Some(world);
        life.state = WorldState::Starting;
    }
    assert!(manager.core_report().world.unwrap().multiplayer_disabled);
    lock(&manager.inner.life)
        .world
        .as_mut()
        .unwrap()
        .options
        .set(O::Multiplayer, 1);
    assert!(!manager.core_report().world.unwrap().multiplayer_disabled);
}

#[test]
fn world_options_reopen_reloads_rules_after_the_previous_server_saves() {
    use launcher::local_worlds::options::WorldOption as O;
    let (runner, save) = FakeRunner::saving();
    let runner = Arc::new(runner);
    let fixture = with_runner(&runner);
    let manager = &fixture.manager;
    let world = manager.create(&flat("Latest rules")).unwrap();
    let level = manager
        .inner
        .store
        .dir(&world.id)
        .unwrap()
        .join(super::super::store::LEVEL_DIR);
    std::fs::create_dir_all(&level).unwrap();
    crate::local_worlds::test_support::write_created_level_dat(&level);
    super::super::level_dat::configure(&world, &level, true).unwrap();
    manager.open(&world.id, None).unwrap();
    wait_for(manager, WorldState::Running);
    manager.close();
    manager.open(&world.id, None).unwrap();
    let mut final_save = world.clone();
    final_save.options.set(O::RandomTickSpeed, 42);
    super::super::level_dat::configure(&final_save, &level, false).unwrap();
    drop(save);
    wait_for(manager, WorldState::Running);
    assert_eq!(
        lock(&runner.started)
            .last()
            .unwrap()
            .options
            .get(O::RandomTickSpeed),
        42
    );
}

#[path = "manager_host_tests.rs"]
mod host;
#[cfg(unix)]
#[path = "manager_leave_tests.rs"]
mod leave;

/// Legacy worlds remain intact and manageable, and never launch a replacement generator.
#[test]
fn legacy_worlds_are_preserved_and_never_started() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = with_runner(&runner);
    let manager = &fixture.manager;
    for backend in [None, Some("dragonfly")] {
        let world = manager.create(&flat("Legacy")).unwrap();
        let directory = manager.inner.store.dir(&world.id).unwrap();
        let metadata = directory.join("world.json");
        let mut record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&metadata).unwrap()).unwrap();
        if let Some(backend) = backend {
            record["backend"] = backend.into();
        } else {
            record.as_object_mut().unwrap().remove("backend");
        }
        std::fs::write(&metadata, record.to_string()).unwrap();
        std::fs::create_dir_all(directory.join("db/db")).unwrap();
        let blocks = directory.join("db/db/blocks.ldb");
        std::fs::write(&blocks, b"preserved blocks").unwrap();
        let before = std::fs::read(&metadata).unwrap();
        assert!(matches!(
            manager.open(&world.id, None),
            Err(WorldError::LegacyWorld)
        ));
        assert_eq!(std::fs::read(&metadata).unwrap(), before);
        assert_eq!(std::fs::read(&blocks).unwrap(), b"preserved blocks");
        assert_eq!(manager.status().state, WorldState::Idle);
        assert!(lock(&runner.started).is_empty());
        assert!(
            manager
                .list()
                .unwrap()
                .iter()
                .any(|saved| saved.id == world.id && saved.backend == Backend::LegacyDragonfly)
        );
        manager
            .update(
                &world.id,
                &WorldUpdate {
                    name: Some("Kept".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            manager.inner.store.get(&world.id).unwrap().backend,
            Backend::LegacyDragonfly
        );
        assert_eq!(std::fs::read(blocks).unwrap(), b"preserved blocks");
    }
}

/// No caller can revive the removed host through an older create request.
#[test]
fn legacy_host_requests_are_refused_without_saving() {
    let fixture = fixture(|_, _| {});
    let result = fixture.manager.create(&NewWorld {
        backend: Some(Backend::LegacyDragonfly),
        ..flat("Legacy")
    });
    assert!(matches!(result, Err(WorldError::LegacyWorld)));
    assert!(fixture.manager.list().unwrap().is_empty());
}
