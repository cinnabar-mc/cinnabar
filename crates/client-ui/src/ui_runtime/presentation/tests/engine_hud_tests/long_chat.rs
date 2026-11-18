use super::*;
use crate::ui_runtime::presentation::forms::{pack_harness, snapshot};

#[test]
fn long_multiline_chat_reaches_painted_hud_and_history() {
    let Some(mut presentation) = engine_presentation_with(pack_harness::font()) else {
        eprintln!("SKIP long chat painting: missing local UI carrier (make assets)");
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    player.inventory.set_local_selected_slot(0);
    let mut runtime = UiRuntime::new(1);
    full_stats(&mut player, &mut runtime, 1);
    let message = ui_screens::test_support::long_chat_message();
    runtime.push_local_chat_line(Arc::from(message.as_str()), 0);
    *presentation.hud_frame_mut() = first_person();
    for focused in [false, true] {
        if focused {
            runtime.open_chat(&mut player);
        }
        let input = build(&player, &mut presentation, &runtime, 100);
        snapshot::write(
            &input,
            if focused {
                "long-chat-history"
            } else {
                "long-chat-hud"
            },
        );
        let nodes = if focused {
            presentation.chat_draw_nodes()
        } else {
            presentation.hud_draw_nodes()
        };
        assert!(
            texts(nodes).contains(&message.as_str()),
            "complete message reaches painting, focused={focused}"
        );
    }
}
