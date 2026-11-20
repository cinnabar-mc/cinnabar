use super::*;

const STARTED_MS: u64 = 1_000;

/// Starts a run as another process would have: no owner to match this test process.
fn begin(root: &Path, level: SafeGraphics) -> Breadcrumb {
    Breadcrumb::begin(root, level, STARTED_MS, None)
}

/// The marker a later launch would read.
fn saved(root: &Path) -> RunMarker {
    match previous(root) {
        Previous::Marker(marker) => marker,
        other => panic!("expected a marker, read {other:?}"),
    }
}

#[test]
fn each_transition_reaches_the_file_and_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(previous(dir.path()), Previous::Missing);
    let mut run = begin(dir.path(), SafeGraphics::SerialCompilation);
    assert_eq!(
        saved(dir.path()),
        RunMarker {
            stage: RunStage::Startup,
            end: None,
            level: 2,
            healthy: false,
            started_ms: STARTED_MS,
            owner: None,
        }
    );
    for stage in [
        RunStage::Menu,
        RunStage::WarmUp,
        RunStage::Menu,
        RunStage::Joining,
        RunStage::InWorld,
    ] {
        run.stage(stage);
        assert_eq!(saved(dir.path()).stage, stage);
        assert_eq!(saved(dir.path()).end, None);
    }
    run.healthy();
    assert!(saved(dir.path()).healthy);
    // Only the marker remains: every write replaced it through a temporary file.
    let files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(files, [std::ffi::OsString::from(FILE)]);
}

#[test]
fn an_unchanged_stage_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = begin(dir.path(), SafeGraphics::Off);
    run.stage(RunStage::Menu);
    std::fs::remove_file(dir.path().join(FILE)).unwrap();
    run.stage(RunStage::Menu);
    run.stage(RunStage::Menu);
    assert_eq!(previous(dir.path()), Previous::Missing);
    run.stage(RunStage::InWorld);
    assert_eq!(saved(dir.path()).stage, RunStage::InWorld);
}

#[test]
fn truncated_corrupt_and_oversized_markers_are_unreadable_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = begin(dir.path(), SafeGraphics::Minimal);
    run.stage(RunStage::WarmUp);
    let path = dir.path().join(FILE);
    let whole = std::fs::read(&path).unwrap();
    for cut in [0, 1, whole.len() / 2, whole.len() - 1] {
        std::fs::write(&path, &whole[..cut]).unwrap();
        assert_eq!(previous(dir.path()), Previous::Unreadable, "cut at {cut}");
    }
    for corrupt in [
        &b"not json"[..],
        br#"{"stage":"orbit"}"#,
        br#"{"end":"clean"}"#,
        br#"{"stage":7}"#,
        br#"[]"#,
    ] {
        std::fs::write(&path, corrupt).unwrap();
        assert_eq!(previous(dir.path()), Previous::Unreadable);
    }
    std::fs::write(&path, vec![b' '; MAX_BYTES as usize + 1]).unwrap();
    assert_eq!(previous(dir.path()), Previous::Unreadable);
    // A marker from a build that knew fewer fields still reads.
    std::fs::write(&path, br#"{"stage":"menu"}"#).unwrap();
    assert_eq!(saved(dir.path()).stage, RunStage::Menu);
    // The next run replaces whatever was there.
    begin(dir.path(), SafeGraphics::Off);
    assert_eq!(saved(dir.path()).stage, RunStage::Startup);
}

#[test]
fn the_strongest_recorded_cause_survives_later_ones() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = begin(dir.path(), SafeGraphics::Off);
    run.end(RunEnd::Panic);
    assert_eq!(saved(dir.path()).end, Some(RunEnd::Panic));
    run.end(RunEnd::Clean);
    assert_eq!(saved(dir.path()).end, Some(RunEnd::Panic));
    run.end(RunEnd::GpuFailure);
    assert_eq!(saved(dir.path()).end, Some(RunEnd::GpuFailure));
    for later in [RunEnd::Clean, RunEnd::Panic, RunEnd::RendererPanic] {
        run.end(later);
        assert_eq!(saved(dir.path()).end, Some(RunEnd::GpuFailure));
    }
    assert!(!saved(dir.path()).healthy);
}

#[test]
fn an_orderly_exit_alone_is_not_a_healthy_session() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = begin(dir.path(), SafeGraphics::Minimal);
    for stage in [RunStage::Menu, RunStage::Joining, RunStage::InWorld] {
        run.stage(stage);
    }
    run.end(RunEnd::Clean);
    assert_eq!(saved(dir.path()).end, Some(RunEnd::Clean));
    assert!(!saved(dir.path()).healthy);

    // The healthy mark comes only from the session itself and outlasts the exit.
    let played = tempfile::tempdir().unwrap();
    let mut run = begin(played.path(), SafeGraphics::Minimal);
    run.stage(RunStage::InWorld);
    run.healthy();
    run.end(RunEnd::Clean);
    assert!(saved(played.path()).healthy);
}

#[test]
fn a_panic_the_process_outlived_is_forgotten_and_a_gpu_failure_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = begin(dir.path(), SafeGraphics::Off);
    for caught in [RunEnd::Panic, RunEnd::RendererPanic] {
        run.end(caught);
        run.survived_panic();
        assert_eq!(saved(dir.path()).end, None, "{caught:?}");
    }
    // A later cause is recorded as if the caught panic had never happened.
    run.end(RunEnd::Clean);
    assert_eq!(saved(dir.path()).end, Some(RunEnd::Clean));
    run.survived_panic();
    assert_eq!(saved(dir.path()).end, Some(RunEnd::Clean));
    run.end(RunEnd::GpuFailure);
    run.survived_panic();
    assert_eq!(saved(dir.path()).end, Some(RunEnd::GpuFailure));
}

#[test]
fn discarding_removes_only_the_marker() {
    let dir = tempfile::tempdir().unwrap();
    discard(dir.path());
    begin(dir.path(), SafeGraphics::Off);
    std::fs::write(dir.path().join("renderer-choice.json"), b"{}").unwrap();
    discard(dir.path());
    assert_eq!(previous(dir.path()), Previous::Missing);
    assert!(dir.path().join("renderer-choice.json").exists());
}

#[test]
fn a_failed_write_does_not_stop_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("removed");
    let mut run = begin(&missing, SafeGraphics::Off);
    run.stage(RunStage::Menu);
    run.end(RunEnd::Clean);
    assert_eq!(previous(&missing), Previous::Missing);
}

#[test]
fn panics_are_renderer_failures_only_from_graphics_sources() {
    for file in [
        "/home/ci/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/wgpu-core-30.0.1/src/device/resource.rs",
        "/home/ci/.cargo/git/checkouts/wgpu-9913bc5f6aacbfdc/12bb53f/wgpu-hal/src/vulkan/device.rs",
        "/home/ci/.cargo/git/checkouts/wgpu-9913bc5f6aacbfdc/12bb53f/wgpu/src/api/surface.rs",
        "/home/ci/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/naga-30.0.0/src/back/spv/writer.rs",
        "/home/ci/.cargo/git/checkouts/bevy-b821f77a6365ded4/5f0dc39/crates/bevy_render/src/view/window/mod.rs",
        "C:\\cargo\\registry\\src\\index.crates.io\\bevy_core_pipeline-0.20.0\\src\\upscaling\\mod.rs",
        "crates/render/src/chunk/draw.rs",
    ] {
        assert_eq!(panic_end(Some(file)), RunEnd::RendererPanic, "{file}");
    }
    for file in [
        "crates/render-scene/src/panorama.rs",
        "crates/render-model/src/actor.rs",
        "crates/client-world/src/lib.rs",
        "app/src/session.rs",
        "/home/ci/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/bevy_ecs-0.20.0/src/system/mod.rs",
        "/rustc/abc/library/core/src/option.rs",
        "crates/wgpuish/src/lib.rs",
    ] {
        assert_eq!(panic_end(Some(file)), RunEnd::Panic, "{file}");
    }
    assert_eq!(panic_end(None), RunEnd::Panic);
}
