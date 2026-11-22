//! The item-in-use build veto reads the crossbow charge item use predicts.
use super::*;

/// Firing a loaded crossbow predicts it empty, so pressing again starts charging before the
/// server restates the stack; that charge vetoes block use although the server's stack is still
/// loaded.
#[test]
fn recharging_after_a_predicted_shot_vetoes_build_actions() {
    let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let (stream, mut ui) = fixture(&mut player_runtime, "minecraft:crossbow");
    publish(
        &mut player_runtime,
        &mut ui,
        2,
        ContainerIdentity::window(0),
        2,
        charged_stack(),
    );
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    let fired = runtime.step(&use_frame(&player_runtime, &stream, &ui, 10));
    assert!(!fired.started, "the loaded crossbow fires");
    runtime.observe_press(true);
    let charging = runtime.step(&use_frame(&player_runtime, &stream, &ui, 40));
    assert!(charging.started, "the predicted-empty crossbow charges");
    assert!(runtime.vetoes_build_action(&player_runtime, &ui, Some(&stream)));
}
