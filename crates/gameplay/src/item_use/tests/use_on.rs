//! Held uses an item's use on a block starts, as a brush's does.
use super::*;

const BRUSH: i32 = 695;
const BRUSH_TICKS: u32 = 200;

/// A frame holding a brush whose press its use on a block consumed.
fn brush_frame(tick: u64, held: bool) -> UseFrame {
    UseFrame {
        selection: Some(selection(2, BRUSH)),
        air_use: None,
        press_consumed: true,
        ..frame(tick, held)
    }
}

#[test]
fn a_use_on_a_block_starts_a_held_use_that_button_up_releases() {
    let mut runtime = ItemUseRuntime::default();
    assert!(runtime.start_use_on(selection(2, BRUSH), BRUSH_TICKS, 0.35, 10));
    assert_eq!(runtime.active_timing(), Some((10, BRUSH_TICKS)));
    assert_eq!(runtime.take_local_using_item(), Some(true));
    assert!(runtime.step(&brush_frame(10, true)).packets.is_empty());

    // A repeated use on the block while the brush is in use leaves that use running.
    assert!(!runtime.start_use_on(selection(2, BRUSH), BRUSH_TICKS, 0.35, 14));
    assert!(runtime.step(&brush_frame(14, true)).packets.is_empty());
    assert_eq!(runtime.active_timing(), Some((10, BRUSH_TICKS)));

    let released = runtime.step(&brush_frame(15, false));
    assert_eq!(kinds(&released), ["release"]);
    assert!(!runtime.is_using());
    assert_eq!(runtime.take_local_using_item(), Some(false));
}

#[test]
fn a_click_on_a_block_releases_its_use_on_the_same_frame() {
    let mut runtime = ItemUseRuntime::default();
    assert!(runtime.start_use_on(selection(2, BRUSH), BRUSH_TICKS, 0.35, 10));
    assert!(runtime.has_work(false));
    assert!(runtime.is_releasing(false));
    assert_eq!(kinds(&runtime.step(&brush_frame(10, false))), ["release"]);
    assert!(!runtime.is_using());
}

#[test]
fn a_use_on_a_block_replaces_a_stale_use_and_its_pending_release() {
    let mut runtime = ItemUseRuntime::default();
    assert!(runtime.start_use_on(selection(3, BRUSH), BRUSH_TICKS, 0.35, 4));
    assert!(runtime.take_local_using_item().is_some());
    // The old use's release was observed but its transaction never went out.
    runtime.observe_use_button(false);
    assert!(runtime.has_pending_release());

    // The server moved the selection to another brush while the old use still ran.
    assert!(runtime.start_use_on(selection(2, BRUSH), BRUSH_TICKS, 0.35, 10));
    assert!(!runtime.has_pending_release());
    assert_eq!(runtime.active_timing(), Some((10, BRUSH_TICKS)));
    assert_eq!(runtime.take_local_using_item(), Some(true));
    assert!(runtime.step(&brush_frame(10, true)).packets.is_empty());
    assert!(runtime.is_using());
}
