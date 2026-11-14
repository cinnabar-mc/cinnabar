//! A server runs only for a world the player opened, and stops once it has kept running for a
//! while with the app off screen.

use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use super::*;
use crate::local_worlds::manager::hidden::{HiddenRun, POLL};

/// A manager over `runner` whose hidden-app watch closes the world after `limit` of running.
fn hidden_after(runner: &Arc<FakeRunner>, limit: Duration) -> Fixture {
    let runner: Arc<dyn Runner> = runner.clone();
    fixture(|parts, _| {
        parts.runner = Some(runner);
        parts.hidden_run_limit = limit;
        parts.hidden_poll = Duration::from_millis(1);
    })
}

/// Listing, creating, editing and reporting worlds start no server; only an open does.
#[test]
fn nothing_starts_without_an_explicit_open() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = with_runner(&runner);
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    manager.list().expect("list");
    manager
        .update(
            &world.id,
            &WorldUpdate {
                name: Some("renamed".to_owned()),
                ..WorldUpdate::default()
            },
        )
        .expect("update");
    let _ = manager.core_report();
    manager.app_hidden();
    assert!(!manager.app_shown());
    assert_eq!(manager.status().state, WorldState::Idle);
    assert_eq!(runner.entered.load(Ordering::SeqCst), 0);
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    assert_eq!(runner.entered.load(Ordering::SeqCst), 1);
}

/// Only time between the watch's wake-ups counts; a frozen process's gap counts as one step.
#[test]
fn hidden_run_time_skips_the_time_the_process_was_frozen() {
    let start = Instant::now();
    let mut run = HiddenRun::new(start, POLL);
    assert_eq!(run.observe(start + POLL), POLL);
    assert_eq!(run.observe(start + POLL * 2), POLL * 2);
    // Frozen for ten minutes between two wake-ups.
    let thawed = start + POLL * 2 + Duration::from_secs(600);
    assert_eq!(run.observe(thawed), POLL * 3);
    assert_eq!(run.observe(thawed + POLL / 2), POLL * 3 + POLL / 2);
    // A clock read out of order adds nothing.
    assert_eq!(run.observe(start), POLL * 3 + POLL / 2);
}

/// A world that keeps running hidden past the limit is saved and stopped, and the return to the
/// app reports that once.
#[test]
fn a_world_running_hidden_past_the_limit_is_saved_and_stopped() {
    let (runner, save) = FakeRunner::saving();
    let runner = Arc::new(runner);
    let fixture = hidden_after(&runner, Duration::ZERO);
    // Dropped before the fixture, whose shutdown waits for the save.
    let save = save;
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    let server = runner.last();
    manager.app_hidden();
    wait_for(manager, WorldState::Stopping);
    assert_eq!(manager.core_report().target, None);
    drop(save);
    wait_for(manager, WorldState::Idle);
    assert!(server.stopped(), "the server was not stopped");
    assert!(manager.closed_while_hidden(), "visible before the return");
    assert!(manager.closed_while_hidden(), "and not consumed by looking");
    assert!(manager.app_shown(), "the return reports the closed world");
    assert!(!manager.app_shown(), "the report is made once");
    assert!(!manager.closed_while_hidden());
    assert_eq!(runner.entered.load(Ordering::SeqCst), 1);
}

/// Returning before the limit keeps the world running, and so does hiding again afterwards.
#[test]
fn returning_before_the_limit_keeps_the_world() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = hidden_after(&runner, Duration::from_secs(3600));
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    for _ in 0..2 {
        manager.app_hidden();
        manager.app_hidden();
        assert!(!manager.app_shown(), "nothing was closed");
        assert_eq!(manager.status().state, WorldState::Running);
    }
    assert!(!runner.last().stopped());
}

/// A start still in progress when the limit passes is cancelled, and a later explicit open still
/// runs the world.
#[test]
fn a_start_in_progress_past_the_limit_is_cancelled() {
    let (runner, release) = FakeRunner::gated();
    let runner = Arc::new(runner);
    let fixture = hidden_after(&runner, Duration::ZERO);
    // Dropped before the fixture, whose shutdown waits for the start.
    let release = release;
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    manager.open(&world.id, None).expect("open");
    test_time::eventually("the start begins", || {
        runner.entered.load(Ordering::SeqCst) == 1
    });
    manager.app_hidden();
    wait_for(manager, WorldState::Idle);
    assert!(lock(&runner.made).is_empty(), "the cancelled start ran");
    assert!(manager.app_shown());
    drop(release);
    manager.open(&world.id, None).expect("explicit reopen");
    wait_for(manager, WorldState::Running);
    assert_eq!(runner.entered.load(Ordering::SeqCst), 2);
}

/// With nothing starting or running a hidden app changes nothing, and a failure stays for the
/// menu to show.
#[test]
fn a_hidden_app_with_nothing_open_changes_nothing() {
    let runner = Arc::new(FakeRunner {
        error: Some(|| WorldError::BackendUnavailable),
        ..FakeRunner::default()
    });
    let fixture = hidden_after(&runner, Duration::ZERO);
    let manager = &fixture.manager;
    manager.app_hidden();
    assert!(!manager.app_shown());
    let world = manager.create(&flat("w")).expect("create");
    manager.open(&world.id, None).expect("open");
    let failed = wait_for(manager, WorldState::Failed);
    manager.app_hidden();
    assert!(!manager.app_shown());
    assert_eq!(manager.status(), failed);
}

/// A world opened after the app was hidden, such as an open still queued when it left the
/// screen, is watched from its open, and so is each later open while still hidden.
#[test]
fn a_world_opened_while_hidden_is_watched_too() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = hidden_after(&runner, Duration::ZERO);
    let manager = &fixture.manager;
    let world = manager.create(&flat("w")).expect("create");
    manager.app_hidden();
    for _ in 0..2 {
        // The second open finds the first watch ended and starts its own.
        manager.open(&world.id, None).expect("open");
        wait_for(manager, WorldState::Idle);
    }
    assert!(runner.entered.load(Ordering::SeqCst) <= 2);
    assert!(manager.app_shown(), "the return reports the closed world");
    manager.open(&world.id, None).expect("reopen on screen");
    wait_for(manager, WorldState::Running);
}
