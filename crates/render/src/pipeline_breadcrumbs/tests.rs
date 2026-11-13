use super::*;
use std::cell::RefCell;

#[test]
fn a_compile_is_named_before_the_driver_call_and_closed_after_it() {
    let lines = RefCell::new(Vec::new());
    let write = |line: &str| lines.borrow_mut().push(line.to_owned());
    let pipeline = bracket(Some(&write), Kind::Compute, Some("terrain cull"), || {
        // A driver fault here leaves the started line as the last one written.
        assert_eq!(
            *lines.borrow(),
            ["Pipeline compile started: compute \"terrain cull\""]
        );
        7
    });
    assert_eq!(pipeline, 7);
    assert_eq!(
        *lines.borrow(),
        [
            "Pipeline compile started: compute \"terrain cull\"",
            "Pipeline compile finished: compute \"terrain cull\"",
        ]
    );
}

#[test]
fn compiles_run_unlogged_without_a_writer_and_unlabelled_pipelines_say_so() {
    assert_eq!(bracket(None, Kind::Render, Some("ui"), || 3), 3);
    let lines = RefCell::new(Vec::new());
    let write = |line: &str| lines.borrow_mut().push(line.to_owned());
    bracket(Some(&write), Kind::Render, None, || ());
    assert_eq!(
        lines.borrow()[0],
        "Pipeline compile started: render \"unlabelled\""
    );
}

/// Advances `compiles` over pipelines in the given stages and returns the lines written.
fn advance(compiles: &mut CachedCompiles, stages: &[fn() -> Stage]) -> Vec<String> {
    let mut lines = Vec::new();
    compiles.advance(
        stages.len(),
        |index| stages[index](),
        |index| {
            let kind = if index == 1 {
                Kind::Compute
            } else {
                Kind::Render
            };
            (kind, format!("pipeline {index}"))
        },
        |line| lines.push(line.to_owned()),
    );
    lines
}

#[test]
fn cached_pipelines_start_once_and_finish_when_their_result_arrives() {
    let pending: fn() -> Stage = || Stage::Pending;
    let ready: fn() -> Stage = || Stage::Ready;
    let mut compiles = CachedCompiles::default();
    assert_eq!(
        advance(&mut compiles, &[pending, pending]),
        [
            "Pipeline compile started: render \"pipeline 0\"",
            "Pipeline compile started: compute \"pipeline 1\"",
        ]
    );
    // Waiting on a worker or a shader writes nothing more.
    assert!(advance(&mut compiles, &[pending, pending]).is_empty());
    assert_eq!(
        advance(&mut compiles, &[pending, ready, pending]),
        [
            "Pipeline compile started: render \"pipeline 2\"",
            "Pipeline compile finished: compute \"pipeline 1\"",
        ]
    );
    assert_eq!(
        advance(&mut compiles, &[ready, ready, ready]),
        [
            "Pipeline compile finished: render \"pipeline 0\"",
            "Pipeline compile finished: render \"pipeline 2\"",
        ]
    );
    assert!(advance(&mut compiles, &[ready, ready, ready]).is_empty());
}

#[test]
fn a_pipeline_compiled_within_the_queue_pass_gets_both_lines() {
    let mut compiles = CachedCompiles::default();
    assert_eq!(
        advance(&mut compiles, &[|| Stage::Ready]),
        [
            "Pipeline compile started: render \"pipeline 0\"",
            "Pipeline compile finished: render \"pipeline 0\"",
        ]
    );
}

#[test]
fn a_rejected_cached_pipeline_reports_its_error_once() {
    let failed: fn() -> Stage = || Stage::Failed("driver rejected the program".into());
    let mut compiles = CachedCompiles::default();
    assert_eq!(
        advance(&mut compiles, &[failed]),
        [
            "Pipeline compile started: render \"pipeline 0\"",
            "Pipeline compile failed: render \"pipeline 0\": driver rejected the program",
        ]
    );
    assert!(advance(&mut compiles, &[failed]).is_empty());
}

#[test]
fn a_rebuilt_cache_is_followed_from_its_first_pipeline() {
    let pending: fn() -> Stage = || Stage::Pending;
    let mut compiles = CachedCompiles::default();
    advance(&mut compiles, &[pending, pending, pending]);
    assert_eq!(
        advance(&mut compiles, &[pending]),
        ["Pipeline compile started: render \"pipeline 0\""]
    );
}
