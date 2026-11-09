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
    runtime.start_use_on(selection(2, BRUSH), BRUSH_TICKS, 0.35);
    let started = runtime.step(&brush_frame(10, true));
    assert!(started.started);
    assert!(started.packets.is_empty());
    assert_eq!(runtime.active_timing(), Some((10, BRUSH_TICKS)));
    assert_eq!(runtime.take_local_using_item(), Some(true));

    // A repeated use on the block while the brush is in use leaves that use running.
    runtime.start_use_on(selection(2, BRUSH), BRUSH_TICKS, 0.35);
    let repeated = runtime.step(&brush_frame(14, true));
    assert!(!repeated.started);
    assert_eq!(runtime.active_timing(), Some((10, BRUSH_TICKS)));

    let released = runtime.step(&brush_frame(15, false));
    assert_eq!(kinds(&released), ["release"]);
    assert!(!runtime.is_using());
    assert_eq!(runtime.take_local_using_item(), Some(false));
}

#[test]
fn a_click_on_a_block_releases_its_use_on_the_next_frame() {
    let mut runtime = ItemUseRuntime::default();
    runtime.start_use_on(selection(2, BRUSH), BRUSH_TICKS, 0.35);
    assert!(runtime.has_work(false));
    assert!(runtime.step(&brush_frame(10, false)).started);
    assert!(runtime.is_releasing(false));
    assert_eq!(kinds(&runtime.step(&brush_frame(10, false))), ["release"]);
    assert!(!runtime.is_using());
}

#[test]
fn a_reselected_slot_drops_a_waiting_use_on_start() {
    let mut runtime = ItemUseRuntime::default();
    runtime.start_use_on(selection(2, BRUSH), BRUSH_TICKS, 0.35);
    let outcome = runtime.step(&UseFrame {
        selection: Some(selection(3, BRUSH)),
        ..brush_frame(10, true)
    });
    assert!(!outcome.started);
    assert!(!runtime.is_using());
    assert!(!runtime.has_work(false));
}
