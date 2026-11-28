use super::*;

/// An unchanged capture repeats; a runtime change, dial frames, animated item frames and the
/// first-person state each make the next capture run. Debug builds check every repeat against
/// a fresh capture.
#[test]
fn unchanged_hud_captures_repeat_and_each_input_runs_again() {
    let mut presentation = UiPresentationRuntime::with_hud(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
    )
    .unwrap();
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let mut capture = |presentation: &mut UiPresentationRuntime,
                       player: &player_state::PlayerState,
                       unchanged: bool,
                       first_person: bool,
                       frames: ItemIconFrames| {
        capture_hud_frame_if_changed(
            player,
            &mut runtime,
            presentation,
            None,
            first_person,
            1_000,
            frames,
            unchanged,
        );
        presentation.hud_captures
    };
    let frames = ItemIconFrames::default();
    let first = capture(&mut presentation, &player, false, true, frames);
    assert_eq!(
        capture(&mut presentation, &player, true, true, frames),
        first
    );
    assert_eq!(
        capture(&mut presentation, &player, true, true, frames),
        first
    );

    player.inventory.set_local_selected_slot(2);
    assert_eq!(
        capture(&mut presentation, &player, false, true, frames),
        first + 1,
        "a runtime change"
    );

    presentation.set_dial_frames(inventory::DialFrames {
        clock: Some(3),
        ..Default::default()
    });
    assert_eq!(
        capture(&mut presentation, &player, true, true, frames),
        first + 2,
        "dial frames"
    );
    let mut animated = frames;
    animated.0[0] = Some(1);
    assert_eq!(
        capture(&mut presentation, &player, true, true, animated),
        first + 3,
        "item frames"
    );
    assert_eq!(
        capture(&mut presentation, &player, true, false, animated),
        first + 4,
        "first person"
    );
    assert!(!presentation.hud_frame().first_person);
    assert_eq!(
        capture(&mut presentation, &player, true, false, animated),
        first + 4
    );
}
