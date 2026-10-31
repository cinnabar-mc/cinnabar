use super::*;
use crate::ui_runtime::presentation::forms::snapshot;

#[test]
fn popup_and_actionbar_remain_visible_in_separate_hud_areas() {
    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping popup HUD layout: missing local UI carrier (make assets)");
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    player.inventory.set_local_selected_slot(0);
    let mut runtime = UiRuntime::new(1);
    full_stats(&mut player, &mut runtime, 1);
    runtime
        .session
        .hud
        .set_actionbar(Arc::from("Action bar"), 2, 0);
    runtime.session.hud.set_popup(Arc::from("Popup"), 3, 0);
    *presentation.hud_frame_mut() = first_person();
    let input = build(&player, &mut presentation, &runtime, 100);
    let nodes = presentation.hud_draw_nodes();
    let popup = text(nodes, "Popup").expect("popup stays visible");
    let actionbar = text(nodes, "Action bar").expect("action bar stays visible");
    assert!(
        actionbar.dest.y + actionbar.dest.h <= popup.dest.y,
        "popup and action bar cannot overlap: {:?}, {:?}",
        popup.dest,
        actionbar.dest
    );
    assert!(
        (popup.dest.x + popup.dest.w * 0.5 - actionbar.dest.x - actionbar.dest.w * 0.5).abs() < 1.0,
        "both messages stay centered"
    );
    snapshot::write(&input, "popup-and-actionbar");
}
