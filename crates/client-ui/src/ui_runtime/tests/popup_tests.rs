use super::*;

/// An authored title request, independent of text-packet popup channels.
fn title(action: protocol::TitleAction, text: &str) -> UiEvent {
    UiEvent::Title(protocol::TitleEvent {
        action,
        text: text.into(),
        document: None,
        fade_in_ticks: 0,
        stay_ticks: 100,
        fade_out_ticks: 0,
        xuid: "".into(),
        platform_online_id: "".into(),
        filtered_message: "".into(),
    })
}

#[test]
fn popup_packets_do_not_replace_or_clear_the_action_bar() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(
            &mut player,
            envelope(1, 1, title(protocol::TitleAction::ActionBar, "Action")),
        )
        .unwrap();
    for (sequence, kind, message) in [
        (2, protocol::TextKind::Popup, "Popup"),
        (3, protocol::TextKind::JukeboxPopup, "Jukebox"),
        (4, protocol::TextKind::Tip, "Tip"),
    ] {
        runtime
            .apply(
                &mut player,
                envelope(
                    1,
                    sequence,
                    UiEvent::Text(protocol::TextEvent {
                        category: protocol::TextCategory::MessageOnly,
                        kind,
                        needs_translation: false,
                        source: None,
                        message: message.into(),
                        parameters: Arc::from([]),
                        xuid: "".into(),
                        platform_chat_id: "".into(),
                        filtered_message: None,
                    }),
                ),
            )
            .unwrap();
    }
    assert_eq!(runtime.hud().actionbar().unwrap().text.as_ref(), "Action");
    assert_eq!(runtime.hud().popup().unwrap().text.as_ref(), "Popup");
    assert_eq!(
        runtime.hud().jukebox_popup().unwrap().text.as_ref(),
        "Jukebox"
    );
    assert_eq!(runtime.hud().tip().unwrap().text.as_ref(), "Tip");
    assert!(runtime.chat().messages().is_empty());

    runtime
        .apply(
            &mut player,
            envelope(1, 5, title(protocol::TitleAction::Clear, "")),
        )
        .unwrap();
    assert!(runtime.hud().actionbar().is_none());
    assert!(runtime.hud().popup().is_some());
    assert!(runtime.hud().jukebox_popup().is_some());
    runtime.begin_session(2);
    assert!(runtime.hud().popup().is_none());
    assert!(runtime.hud().jukebox_popup().is_none());
}
