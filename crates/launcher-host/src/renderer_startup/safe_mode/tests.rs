use super::*;

const VERSION: &str = "1.2.3";
const STARTED_MS: u64 = 10_000;
/// `ApplicationExitInfo.REASON_CRASH_NATIVE` with `SIGSEGV`.
const NATIVE_CRASH: (i32, i32) = (5, 11);
/// `ApplicationExitInfo.REASON_USER_REQUESTED`: the player swiped the app away.
const SWIPED_AWAY: (i32, i32) = (10, 0);
/// `ApplicationExitInfo.REASON_LOW_MEMORY`.
const LOW_MEMORY: (i32, i32) = (3, 0);
/// `ApplicationExitInfo.REASON_ANR`.
const NOT_RESPONDING: (i32, i32) = (6, 0);

/// A marker as a run at `stage` would leave it.
fn marker(stage: RunStage, end: Option<RunEnd>, healthy: bool) -> RunMarker {
    RunMarker {
        stage,
        end,
        level: 0,
        healthy,
        started_ms: STARTED_MS,
        owner: None,
    }
}

/// An exit Android recorded after the run began.
fn exit((reason, status): (i32, i32)) -> Option<ExitRecord> {
    Some(ExitRecord {
        reason,
        status,
        timestamp_ms: STARTED_MS + 5_000,
    })
}

/// Leaves the marker of a run that another process made at the ladder's saved level.
fn previous_run(root: &Path, level: SafeGraphics, run: impl FnOnce(&mut Breadcrumb)) {
    let mut breadcrumb = Breadcrumb::begin(root, level, STARTED_MS, None);
    run(&mut breadcrumb);
}

/// The next launch after a run that Android recorded as `exit`.
fn launch(root: &Path, exit: Option<ExitRecord>) -> SafeStart {
    start(
        root,
        Launch {
            version: VERSION,
            graphics_api_revision: 0,
            exit,
            now_ms: STARTED_MS + 60_000,
        },
    )
}

const STAGES: [RunStage; 5] = [
    RunStage::Startup,
    RunStage::Menu,
    RunStage::WarmUp,
    RunStage::Joining,
    RunStage::InWorld,
];

#[test]
fn a_native_fault_counts_at_every_stage_and_other_exits_never_do() {
    assert_eq!(outcome(None, ExitKind::NativeCrash), Outcome::Inconclusive);
    for stage in STAGES {
        let vanished = marker(stage, None, false);
        assert_eq!(
            outcome(Some(&vanished), ExitKind::NativeCrash),
            Outcome::RendererDeath,
            "{stage:?}"
        );
        // A swipe-away, a low-memory kill, an unresponsive app or a Java crash.
        assert_eq!(
            outcome(Some(&vanished), ExitKind::Other),
            Outcome::Inconclusive,
            "{stage:?}"
        );
    }
}

#[test]
fn without_an_exit_record_only_compile_stages_count() {
    for stage in STAGES {
        let expected = if matches!(stage, RunStage::Startup | RunStage::WarmUp) {
            Outcome::RendererDeath
        } else {
            Outcome::Inconclusive
        };
        assert_eq!(
            outcome(Some(&marker(stage, None, false)), ExitKind::Unknown),
            expected,
            "{stage:?}"
        );
    }
}

#[test]
fn a_cause_the_run_recorded_decides_whatever_the_platform_says() {
    for exit in [ExitKind::Unknown, ExitKind::NativeCrash, ExitKind::Other] {
        for stage in STAGES {
            for (end, expected) in [
                (RunEnd::GpuFailure, Outcome::RendererDeath),
                (RunEnd::RendererPanic, Outcome::RendererDeath),
                // An ordinary crash and an orderly exit are not the renderer's.
                (RunEnd::Panic, Outcome::Inconclusive),
                (RunEnd::Clean, Outcome::Inconclusive),
            ] {
                assert_eq!(
                    outcome(Some(&marker(stage, Some(end), false)), exit),
                    expected,
                    "{stage:?} {end:?} {exit:?}"
                );
            }
        }
    }
}

#[test]
fn a_healthy_session_counts_unless_the_renderer_then_dies() {
    let healthy = |end| marker(RunStage::InWorld, end, true);
    for end in [None, Some(RunEnd::Clean), Some(RunEnd::Panic)] {
        assert_eq!(
            outcome(Some(&healthy(end)), ExitKind::Other),
            Outcome::Healthy
        );
    }
    assert_eq!(
        outcome(Some(&healthy(None)), ExitKind::NativeCrash),
        Outcome::RendererDeath
    );
    assert_eq!(
        outcome(Some(&healthy(Some(RunEnd::GpuFailure))), ExitKind::Other),
        Outcome::RendererDeath
    );
}

#[test]
fn repeated_deaths_climb_the_ladder_one_level_at_a_time_and_stop_at_the_top() {
    let mut ladder = Ladder::default();
    for expected in SafeGraphics::ALL.into_iter().skip(1) {
        ladder = ladder.after(Outcome::RendererDeath);
        assert_eq!(ladder.level, expected);
        assert_eq!(ladder.after(Outcome::Inconclusive), ladder);
        assert!(!ladder.pinned);
    }
    assert_eq!(
        ladder.after(Outcome::RendererDeath).level,
        SafeGraphics::MAX
    );
}

#[test]
fn a_healthy_session_clears_every_level() {
    for level in SafeGraphics::ALL {
        let cleared = Ladder {
            level,
            ..Ladder::default()
        }
        .after(Outcome::Healthy);
        assert_eq!(cleared.level, SafeGraphics::Off);
        assert!(!cleared.pinned);
        assert_eq!(cleared.cleared_from, level.is_active().then_some(level));
    }
}

#[test]
fn a_death_straight_after_a_clear_returns_to_the_level_that_worked_and_stays() {
    let worked = SafeGraphics::SerialCompilation;
    let cleared = Ladder {
        level: worked,
        ..Ladder::default()
    }
    .after(Outcome::Healthy);
    let relapsed = cleared.after(Outcome::RendererDeath);
    assert_eq!(
        relapsed,
        Ladder {
            level: worked,
            cleared_from: None,
            pinned: true
        }
    );
    // Healthy sessions no longer clear it, and a death at that level still steps down.
    assert_eq!(relapsed.after(Outcome::Healthy), relapsed);
    let worse = relapsed.after(Outcome::RendererDeath);
    assert_eq!(worse.level, SafeGraphics::Minimal);
    assert!(worse.pinned);
}

#[test]
fn full_graphics_that_prove_themselves_forget_the_cleared_level() {
    let cleared = Ladder {
        level: SafeGraphics::Minimal,
        ..Ladder::default()
    }
    .after(Outcome::Healthy);
    let proven = cleared.after(Outcome::Healthy);
    assert_eq!(proven, Ladder::default());
    // A later death starts at the first step again, not at the old level.
    let later = proven.after(Outcome::RendererDeath);
    assert_eq!(later.level, SafeGraphics::FreshPipelines);
    assert!(!later.pinned);
}

#[test]
fn launches_step_down_after_each_renderer_death_and_clear_after_a_healthy_session() {
    let dir = tempfile::tempdir().unwrap();
    let first = launch(dir.path(), None);
    assert_eq!(first.level, SafeGraphics::Off);

    // Dies compiling world pipelines behind the menu, as Android reports a native crash.
    previous_run(dir.path(), SafeGraphics::Off, |run| {
        run.stage(RunStage::Menu);
        run.stage(RunStage::WarmUp);
    });
    let second = launch(dir.path(), exit(NATIVE_CRASH));
    assert_eq!(second.level, SafeGraphics::FreshPipelines);
    assert!(second.reason.contains("warm_up"), "{}", second.reason);
    assert!(
        second.reason.contains("reason 5 status 11"),
        "{}",
        second.reason
    );

    // Loses the device in the world and exits by itself.
    previous_run(dir.path(), second.level, |run| {
        run.stage(RunStage::InWorld);
        run.end(RunEnd::GpuFailure);
        run.end(RunEnd::Clean);
    });
    let third = launch(dir.path(), exit((1, 1)));
    assert_eq!(third.level, SafeGraphics::SerialCompilation);

    // The player swipes the app away from the menu: the level neither rises nor clears.
    previous_run(dir.path(), third.level, |run| run.stage(RunStage::Menu));
    let fourth = launch(dir.path(), exit(SWIPED_AWAY));
    assert_eq!(fourth.level, SafeGraphics::SerialCompilation);

    // Survives a session, then is swiped away.
    previous_run(dir.path(), fourth.level, |run| {
        run.stage(RunStage::InWorld);
        run.healthy();
    });
    let fifth = launch(dir.path(), exit(SWIPED_AWAY));
    assert_eq!(fifth.level, SafeGraphics::Off);

    // Full graphics die again at once: back to the level that worked, for good.
    previous_run(dir.path(), fifth.level, |run| run.stage(RunStage::WarmUp));
    let sixth = launch(dir.path(), exit(NATIVE_CRASH));
    assert_eq!(sixth.level, SafeGraphics::SerialCompilation);
    previous_run(dir.path(), sixth.level, |run| {
        run.stage(RunStage::InWorld);
        run.healthy();
    });
    assert_eq!(
        launch(dir.path(), exit(SWIPED_AWAY)).level,
        SafeGraphics::SerialCompilation
    );
}

#[test]
fn exits_that_are_not_the_renderer_leave_full_graphics() {
    for (stage, end, exit_record) in [
        (RunStage::InWorld, None, exit(SWIPED_AWAY)),
        (RunStage::InWorld, None, exit(LOW_MEMORY)),
        (RunStage::Joining, None, exit(NOT_RESPONDING)),
        (RunStage::Menu, Some(RunEnd::Clean), exit((1, 0))),
        // A panic in game code aborts the process, which Android reports as a native crash.
        (RunStage::Joining, Some(RunEnd::Panic), exit((5, 6))),
        // No exit record at all, as before Android 11, outside the compile stages.
        (RunStage::Menu, None, None),
    ] {
        let dir = tempfile::tempdir().unwrap();
        launch(dir.path(), None);
        previous_run(dir.path(), SafeGraphics::Off, |run| {
            run.stage(stage);
            if let Some(end) = end {
                run.end(end);
            }
        });
        assert_eq!(
            launch(dir.path(), exit_record).level,
            SafeGraphics::Off,
            "{stage:?} {end:?} {exit_record:?}"
        );
    }
}

#[test]
fn an_exit_record_from_before_the_run_is_not_its_cause() {
    let dir = tempfile::tempdir().unwrap();
    launch(dir.path(), None);
    previous_run(dir.path(), SafeGraphics::Off, |run| {
        run.stage(RunStage::InWorld)
    });
    let stale = Some(ExitRecord {
        reason: NATIVE_CRASH.0,
        status: NATIVE_CRASH.1,
        timestamp_ms: STARTED_MS - 1,
    });
    assert_eq!(launch(dir.path(), stale).level, SafeGraphics::Off);
}

#[test]
fn a_second_run_in_one_process_is_not_a_death_of_the_first() {
    let dir = tempfile::tempdir().unwrap();
    // The first launch leaves its own marker at startup, with no exit record to explain it.
    assert_eq!(launch(dir.path(), None).level, SafeGraphics::Off);
    let again = launch(dir.path(), None);
    assert_eq!(again.level, SafeGraphics::Off);
    assert!(again.reason.contains("this process"), "{}", again.reason);
}

#[test]
fn a_new_version_or_graphics_api_choice_clears_the_ladder() {
    let climbed = |root: &Path| {
        launch(root, None);
        for _ in 0..2 {
            previous_run(root, SafeGraphics::Off, |run| run.stage(RunStage::WarmUp));
            launch(root, exit(NATIVE_CRASH));
        }
        previous_run(root, SafeGraphics::Off, |run| run.stage(RunStage::WarmUp));
    };
    let updated = tempfile::tempdir().unwrap();
    climbed(updated.path());
    let start_updated = start(
        updated.path(),
        Launch {
            version: "1.2.4",
            graphics_api_revision: 0,
            exit: exit(NATIVE_CRASH),
            now_ms: STARTED_MS,
        },
    );
    assert_eq!(start_updated.level, SafeGraphics::Off);

    let chosen = tempfile::tempdir().unwrap();
    climbed(chosen.path());
    let start_chosen = start(
        chosen.path(),
        Launch {
            version: VERSION,
            graphics_api_revision: 1,
            exit: exit(NATIVE_CRASH),
            now_ms: STARTED_MS,
        },
    );
    assert_eq!(start_chosen.level, SafeGraphics::Off);
    assert!(
        start_chosen.reason.contains("cleared"),
        "{}",
        start_chosen.reason
    );

    let unchanged = tempfile::tempdir().unwrap();
    climbed(unchanged.path());
    assert_eq!(
        launch(unchanged.path(), exit(NATIVE_CRASH)).level,
        SafeGraphics::Minimal
    );
}

#[test]
fn unreadable_state_starts_at_full_graphics_and_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(FILE), b"{\"level\":").unwrap();
    std::fs::write(dir.path().join(breadcrumb::FILE), b"{\"stage\":\"wa").unwrap();
    assert_eq!(
        launch(dir.path(), exit(NATIVE_CRASH)).level,
        SafeGraphics::Off
    );
    // Both files are whole again, so the next death is judged normally.
    previous_run(dir.path(), SafeGraphics::Off, |run| {
        run.stage(RunStage::WarmUp)
    });
    assert_eq!(
        launch(dir.path(), exit(NATIVE_CRASH)).level,
        SafeGraphics::FreshPipelines
    );

    // A truncated marker under a readable ladder is evidence of nothing.
    std::fs::write(dir.path().join(breadcrumb::FILE), b"{\"stage\":\"wa").unwrap();
    let kept = launch(dir.path(), exit(NATIVE_CRASH));
    assert_eq!(kept.level, SafeGraphics::FreshPipelines);
    assert!(kept.reason.contains("unreadable"), "{}", kept.reason);
}

#[test]
fn a_saved_level_past_the_ladder_is_its_last_step() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(FILE),
        format!(r#"{{"level":9,"version":"{VERSION}"}}"#),
    )
    .unwrap();
    assert_eq!(launch(dir.path(), None).level, SafeGraphics::MAX);
}

#[test]
fn each_level_is_announced_once_until_it_changes() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!launch(dir.path(), None).notify);
    previous_run(dir.path(), SafeGraphics::Off, |run| {
        run.stage(RunStage::WarmUp)
    });
    let first = launch(dir.path(), exit(NATIVE_CRASH));
    assert!(first.notify && !first.kept);
    // The same level again, after a run that proved nothing, stays quiet.
    previous_run(dir.path(), first.level, |run| run.stage(RunStage::Menu));
    assert!(!launch(dir.path(), exit(SWIPED_AWAY)).notify);
    // A further step is news.
    previous_run(dir.path(), first.level, |run| run.stage(RunStage::WarmUp));
    let second = launch(dir.path(), exit(NATIVE_CRASH));
    assert_eq!(second.level, SafeGraphics::SerialCompilation);
    assert!(second.notify);
    // So is returning to a level after full graphics failed again, now kept.
    previous_run(dir.path(), second.level, |run| {
        run.stage(RunStage::InWorld);
        run.healthy();
    });
    assert!(!launch(dir.path(), exit(SWIPED_AWAY)).notify);
    previous_run(dir.path(), SafeGraphics::Off, |run| {
        run.stage(RunStage::WarmUp)
    });
    let kept = launch(dir.path(), exit(NATIVE_CRASH));
    assert_eq!(kept.level, SafeGraphics::SerialCompilation);
    assert!(kept.notify && kept.kept);
    assert_eq!(SafeStart::unused().level, SafeGraphics::Off);
}

#[test]
fn a_forced_level_saves_nothing_and_drops_the_previous_marker() {
    let dir = tempfile::tempdir().unwrap();
    launch(dir.path(), None);
    previous_run(dir.path(), SafeGraphics::Off, |run| {
        run.stage(RunStage::Menu)
    });
    let ladder = std::fs::read(dir.path().join(FILE)).unwrap();
    let forced = SafeStart::forced(dir.path(), SafeGraphics::Minimal);
    assert_eq!(forced.level, SafeGraphics::Minimal);
    assert!(forced.notify && !forced.kept);
    assert!(!SafeStart::forced(dir.path(), SafeGraphics::Off).notify);
    assert_eq!(std::fs::read(dir.path().join(FILE)).unwrap(), ladder);
    // The forced run's native crash is not attributed to the older run's menu marker.
    let after = launch(dir.path(), exit(NATIVE_CRASH));
    assert_eq!(after.level, SafeGraphics::Off);
    assert!(after.reason.contains("no earlier run"), "{}", after.reason);
}
