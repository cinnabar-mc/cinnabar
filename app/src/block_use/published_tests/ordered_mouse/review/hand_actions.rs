//! Physical hand-button arbitration through the production interaction schedule.
use super::*;

/// Starts a survival destroy with a held left button and drains its first carrier.
fn mining_fixture() -> (App, client_session::CapturedPackets) {
    let (mut app, mut captured) = app_fixture(16);
    remove_actor(app.world_mut());
    set_hotbar_stack(app.world_mut(), 0, Some("minecraft:stone"));
    app.world_mut()
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    world_pick_for_setup(app.world_mut());
    mouse_frame(app.world_mut(), MouseButton::Left, &[ButtonState::Pressed]);
    attack_progress_tests::ticks(app.world_mut(), 102, 103);
    frame(&mut app);
    assert!(destroying(&app));
    captured.drain();
    (app, captured)
}

/// Reads the observable mining owner's current target.
fn destroying(app: &App) -> bool {
    app.world()
        .resource::<client_interaction::survival_mining::SurvivalMiningRuntime>()
        .destroying_target()
        .is_some()
}

#[test]
fn ordered_mouse_hand_actions_use_release_does_not_resume_the_older_attack() {
    let (mut app, mut captured) = mining_fixture();
    mouse_frame(app.world_mut(), MouseButton::Right, &[ButtonState::Pressed]);
    attack_progress_tests::ticks(app.world_mut(), 103, 104);
    frame(&mut app);
    assert!(!destroying(&app));
    captured.drain();
    mouse_frame(
        app.world_mut(),
        MouseButton::Right,
        &[ButtonState::Released],
    );
    attack_progress_tests::ticks(app.world_mut(), 104, 105);
    frame(&mut app);
    assert!(
        !destroying(&app),
        "releasing use must not restore the old attack"
    );
    assert!(
        captured
            .drain()
            .iter()
            .flat_map(protocol::test_support::packets::input_block_actions)
            .all(|action| action.0 != protocol::BlockActionKind::StartDestroy.wire_value())
    );
}

#[test]
fn ordered_mouse_hand_actions_new_attack_wins_while_use_remains_held() {
    let (mut app, mut captured) = mining_fixture();
    mouse_frame(app.world_mut(), MouseButton::Right, &[ButtonState::Pressed]);
    attack_progress_tests::ticks(app.world_mut(), 103, 104);
    frame(&mut app);
    captured.drain();
    mouse_frame(
        app.world_mut(),
        MouseButton::Left,
        &[ButtonState::Released, ButtonState::Pressed],
    );
    attack_progress_tests::ticks(app.world_mut(), 104, 105);
    frame(&mut app);
    captured.drain();
    attack_progress_tests::ticks(app.world_mut(), 105, 106);
    frame(&mut app);
    assert!(
        destroying(&app),
        "a newer attack must continue with both buttons held"
    );
}
