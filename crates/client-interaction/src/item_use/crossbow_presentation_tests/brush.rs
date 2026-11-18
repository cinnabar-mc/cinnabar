//! A brush's local use drives the local rig's use flag in both directions.

use super::*;

/// A brush started using from a block and then released clears the local use flag, so the
/// brushing pose drops without waiting for the server's flag.
#[test]
fn a_released_brush_clears_the_local_use_flag() {
    let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let (stream, ui) = fixture(&mut player_runtime, assets::facts::BRUSH_IDENTIFIER);
    let mut runtime = ItemUseRuntime::default();
    assert_eq!(
        runtime.local_item_use(&player_runtime, &stream, &ui),
        LocalItemUse::Idle
    );

    assert!(runtime.start_use_on(selection(2, BOW), 200, 0.35, 10));
    assert_eq!(
        runtime.local_item_use(&player_runtime, &stream, &ui),
        LocalItemUse::Using
    );

    let released = runtime.step(&UseFrame {
        air_use: None,
        press_consumed: true,
        ..frame(11, false)
    });
    assert_eq!(kinds(&released), ["release"]);
    assert_eq!(
        runtime.local_item_use(&player_runtime, &stream, &ui),
        LocalItemUse::Idle
    );
}
