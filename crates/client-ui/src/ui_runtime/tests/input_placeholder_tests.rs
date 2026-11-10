use super::*;

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

fn text(kind: protocol::TextKind, message: &str) -> UiEvent {
    UiEvent::Text(protocol::TextEvent {
        category: protocol::TextCategory::MessageOnly,
        kind,
        needs_translation: false,
        source: None,
        message: Arc::from(message),
        parameters: Arc::from([]),
        xuid: Arc::from(""),
        platform_chat_id: Arc::from(""),
        filtered_message: None,
    })
}

/// Server text naming `:_input_key.forward:` shows the player's bound key, as vanilla
/// substitutes it for titles, the action bar and chat; tips and raw text keep the placeholder.
#[test]
fn input_placeholders_name_the_bound_control_where_vanilla_expands_them() {
    const HINT: &str = "Hold :_input_key.forward: to walk";
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let events = [
        title(protocol::TitleAction::SetTitle, HINT),
        title(protocol::TitleAction::SetSubtitle, HINT),
        title(protocol::TitleAction::ActionBar, HINT),
        text(protocol::TextKind::Tip, HINT),
        text(protocol::TextKind::Chat, HINT),
        text(protocol::TextKind::Raw, HINT),
    ];
    for (sequence, event) in (1..).zip(events) {
        runtime
            .apply(&mut player_runtime, envelope(1, sequence, event))
            .unwrap();
    }

    let walk = "Hold W to walk";
    let hud = runtime.hud();
    assert_eq!(hud.title().unwrap().text.as_ref(), walk);
    assert_eq!(hud.subtitle().unwrap().text.as_ref(), walk);
    assert_eq!(hud.actionbar().unwrap().text.as_ref(), walk);
    assert_eq!(hud.tip().unwrap().text.as_ref(), HINT);
    let chat: Vec<_> = runtime
        .chat()
        .messages()
        .iter()
        .map(|row| row.message.as_ref())
        .collect();
    assert_eq!(chat, [walk, HINT]);
}

/// The label follows the player's current bindings and input mode when the text arrives.
#[test]
fn input_placeholders_follow_remaps_and_the_gamepad() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let mut settings = launcher::menu::settings_options::SettingsOptions::default();
    let jump = launcher::menu::settings_options::BindingId::Jump;
    assert!(settings.remap(
        jump.row(false).unwrap(),
        semantic_input::PhysicalControl::KeyboardUsage(0x0e),
    ));
    runtime.set_control_settings(Arc::new(settings));
    let bar = ":_input_key.jump: / :_input_key.forward:";
    runtime
        .apply(
            &mut player_runtime,
            envelope(1, 1, title(protocol::TitleAction::ActionBar, bar)),
        )
        .unwrap();
    assert_eq!(runtime.hud().actionbar().unwrap().text.as_ref(), "K / W");

    runtime.set_input_mode(semantic_input::InputMode::GamePad);
    runtime
        .apply(
            &mut player_runtime,
            envelope(1, 2, title(protocol::TitleAction::ActionBar, bar)),
        )
        .unwrap();
    assert_eq!(
        runtime.hud().actionbar().unwrap().text.as_ref(),
        "A / controllerLayoutScreen.unassigned"
    );
}

fn server_lang(runtime: &mut UiRuntime, entries: &str) {
    let input = entries.as_bytes();
    runtime.set_server_lang(assets::ServerLangOverlay::read(input.len(), |target| {
        target.copy_from_slice(input);
        true
    }));
}

/// A translated chat row is expanded once localized, as vanilla localizes it before expanding.
#[test]
fn translated_chat_expands_the_localized_line() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    server_lang(
        &mut runtime,
        "hint=Press :_input_key.jump: to %s\nplain=No hint\n",
    );
    let mut hint = text(protocol::TextKind::Translation, "hint");
    if let UiEvent::Text(event) = &mut hint {
        event.parameters = Arc::from([Arc::from("fly")]);
    }
    for (sequence, event) in (1..).zip([hint, text(protocol::TextKind::Translation, "plain")]) {
        runtime
            .apply(&mut player_runtime, envelope(1, sequence, event))
            .unwrap();
    }
    let rows: Vec<_> = runtime
        .chat()
        .messages()
        .iter()
        .map(|row| (row.kind, row.message.as_ref()))
        .collect();
    assert_eq!(
        rows,
        [
            (ui::ChatMessageKind::System, "Press Space to fly"),
            (ui::ChatMessageKind::Translation, "plain"),
        ]
    );
}

/// A long label repeated by many placeholders skips the row instead of building it.
#[test]
fn oversized_expansions_are_skipped() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let label = "x".repeat(1024);
    server_lang(
        &mut runtime,
        &format!("controllerLayoutScreen.unassigned={label}\n"),
    );
    let flood = ":_input_:".repeat(2048);
    runtime
        .apply(
            &mut player_runtime,
            envelope(1, 1, text(protocol::TextKind::System, &flood)),
        )
        .unwrap();
    assert!(runtime.chat().messages().is_empty());
    runtime
        .apply(
            &mut player_runtime,
            envelope(1, 2, title(protocol::TitleAction::ActionBar, &flood)),
        )
        .unwrap();
    assert_eq!(runtime.hud().actionbar().unwrap().text.as_ref(), flood);
}
