use super::*;

/// Chat and the other text boxes report that they take typed characters; an idle HUD does not.
#[test]
fn text_entry_is_focused_in_chat_and_in_each_screen_text_box() {
    let mut player_runtime = player_state::PlayerState::new(2);
    let mut runtime = UiRuntime::new(2);
    assert!(!runtime.text_entry_focused());

    runtime.open_chat(&mut player_runtime);
    assert!(runtime.text_entry_focused());

    let mut runtime = UiRuntime::new(2);
    runtime.screen_state_mut().search_focused = true;
    assert!(runtime.text_entry_focused());

    let mut runtime = UiRuntime::new(2);
    runtime.screen_state_mut().anvil_focused = true;
    assert!(runtime.text_entry_focused());
}

#[test]
fn tab_cycles_whitespace_suggestions_as_complete_replacements() {
    let mut player_runtime = player_state::PlayerState::new(2);
    let mut runtime = UiRuntime::new(2);
    runtime.open_chat(&mut player_runtime);
    runtime.insert_chat_text("/tell A").unwrap();
    let request = runtime.take_chat_autocomplete_request().unwrap();
    runtime
        .session
        .chat_autocomplete
        .apply(
            request,
            ui::ChatAutocompleteDelta {
                enum_name: Arc::from("players"),
                action: ui::ChatAutocompleteAction::Replace,
                suggestions: Arc::from([Arc::from("Alice One"), Arc::from("Alice Two")]),
            },
        )
        .unwrap();
    assert!(runtime.handle_chat_ui_action(UiAction::TabNext));
    assert_eq!(runtime.chat_editor().as_str(), "/tell Alice One");
    assert!(runtime.handle_chat_ui_action(UiAction::TabNext));
    assert_eq!(runtime.chat_editor().as_str(), "/tell Alice Two");
}
