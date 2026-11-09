use super::*;

/// Builds an ordinary received chat line without localization or local asset dependencies.
fn chat_event(message: &str) -> UiEvent {
    UiEvent::Text(protocol::TextEvent {
        category: protocol::TextCategory::MessageOnly,
        kind: protocol::TextKind::Raw,
        needs_translation: false,
        source: None,
        message: message.into(),
        parameters: Arc::from([]),
        xuid: "".into(),
        platform_chat_id: "".into(),
        filtered_message: None,
    })
}

/// Admits a small modal through the same store used by committed form packets.
fn admit_form(runtime: &mut UiRuntime) -> ServerFormIdentity {
    let session = runtime.session_id();
    runtime.server_forms_mut().admit(
        protocol::FormRequestEvent {
            form_id: 4,
            kind: protocol::FormKind::Modal,
            title: None,
            json: Arc::from("{}"),
            model: protocol::ServerFormModel::Modal(protocol::ModalDialogForm {
                title: "Question".into(),
                content: "Continue?".into(),
                button1: "Yes".into(),
                button2: "No".into(),
            }),
        },
        3,
        session,
        false,
    );
    runtime.server_forms().active().unwrap().identity
}

/// Populates independent receive, input and send owners without requiring local carriers.
fn populated() -> (player_state::PlayerState, UiRuntime) {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(&mut player, envelope(1, 1, chat_event("Hello")))
        .unwrap();
    runtime
        .apply(
            &mut player,
            envelope(1, 2, UiEvent::Hud(HudEvent::Health { health: 17 })),
        )
        .unwrap();
    runtime.publish_inventory_authority(&mut player, protocol::InventoryAuthority::Server);
    runtime.toggle_inventory(&mut player, None);
    runtime.set_inventory_pointer_gui(Some([12.0, 34.0]));
    runtime.screen_state_mut().search = "old search".into();
    let mut book = book_screen::BookState::new(
        book_screen::BookSource::Held(0),
        vec!["before".into()],
        true,
        String::new(),
        String::new(),
    );
    book.type_text("after");
    runtime.open_book(book);
    assert!(runtime.commit_book(false));
    runtime.queue_chat_message("first", 30).unwrap();
    runtime.queue_chat_message("second", 31).unwrap();
    let sequence = runtime.pending_chat_sends()[0].sequence;
    assert!(runtime.mark_chat_send_enqueued(1, sequence));
    runtime.insert_chat_text("/tell A").unwrap();
    admit_form(&mut runtime);
    assert!(
        runtime.queue_client_packet(protocol::modal_form_submit_response(
            8,
            protocol::ModalFormResponseSelection::ModalButton(true),
        ))
    );
    (player, runtime)
}

#[test]
fn same_session_preserves_receive_input_and_pending_sends() {
    let (mut player, mut runtime) = populated();
    let form = runtime.server_forms().active().unwrap().identity;
    let generation = runtime.item_name_generation();
    let revision = runtime.scoreboards().revision();
    runtime.begin_session(1);

    assert_eq!(runtime.chat().messages().len(), 1);
    assert_eq!(runtime.hud().health().unwrap().current(), 17);
    assert!(runtime.inventory_open());
    assert_eq!(runtime.inventory_pointer_gui(), Some([12.0, 34.0]));
    assert_eq!(runtime.screen_state().search, "old search");
    assert_eq!(runtime.chat_editor().as_str(), "/tell A");
    assert!(runtime.take_chat_autocomplete_request().is_some());
    assert_eq!(runtime.server_forms().active().unwrap().identity, form);
    assert_eq!(runtime.pending_chat_sends().len(), 2);
    assert_eq!(runtime.in_flight_chat_sends(), 1);
    assert_eq!(runtime.dropped_unsent_chat_messages(), 0);
    assert!(runtime.take_client_packet().is_some());
    assert!(runtime.take_client_packet().is_some());
    assert!(runtime.take_client_packet().is_none());
    assert_eq!(runtime.item_name_generation(), generation);
    assert_eq!(runtime.scoreboards().revision(), revision);
    assert!(matches!(
        runtime.apply(
            &mut player,
            envelope(1, 2, UiEvent::Hud(HudEvent::Health { health: 20 }))
        ),
        Err(UiRuntimeError::StaleFifoSequence { .. })
    ));
}

#[test]
fn switching_sessions_retires_receive_input_and_pending_sends() {
    let (mut player, mut runtime) = populated();
    let old_row = runtime.chat().observed_rows().next().unwrap().0;
    let form = runtime.server_forms().active().unwrap().identity;
    runtime
        .respond_to_server_form(form, LocalFormAction::SubmitButton(0))
        .unwrap();
    runtime.open_chat(&mut player);
    runtime.begin_session(2);
    player.begin_session(2);

    assert_eq!(runtime.session_id(), 2);
    assert!(runtime.chat().messages().is_empty());
    assert!(runtime.hud().health().is_none());
    assert!(!runtime.chat_focused());
    assert!(!runtime.inventory_open());
    assert_eq!(runtime.inventory_pointer_gui(), None);
    assert!(runtime.screen_state().search.is_empty());
    assert!(runtime.chat_editor().as_str().is_empty());
    assert!(runtime.chat_suggestions().is_empty());
    assert!(runtime.take_chat_autocomplete_request().is_none());
    assert!(runtime.server_forms().active().is_none());
    assert!(!flush_form_response(&mut runtime, |_| false, |_| Ok(())).unwrap());
    assert!(runtime.pending_chat_sends().is_empty());
    assert_eq!(runtime.in_flight_chat_sends(), 0);
    assert_eq!(runtime.dropped_unsent_chat_messages(), 2);
    assert!(runtime.take_client_packet().is_none());
    assert!(!runtime.acknowledge_chat_send(1, 0));
    assert!(matches!(
        runtime.apply(
            &mut player,
            envelope(1, 3, UiEvent::Hud(HudEvent::Health { health: 20 }))
        ),
        Err(UiRuntimeError::WrongSession { .. })
    ));
    runtime
        .apply(&mut player, envelope(2, 1, chat_event("New")))
        .unwrap();
    assert!(runtime.chat().observed_rows().next().unwrap().0 > old_row);
    let request = runtime.queue_chat_message("new server", 0).unwrap();
    assert_eq!((request.session, request.sequence), (2, 0));
    assert_eq!(runtime.dropped_unsent_chat_messages(), 2);
    runtime.begin_session(3);
    assert_eq!(runtime.dropped_unsent_chat_messages(), 3);
    let next_form = admit_form(&mut runtime);
    assert!(next_form.revision > form.revision);
}

#[test]
fn switching_sessions_preserves_local_language_history_and_cache_identity() {
    let (_, mut runtime) = populated();
    let language = |value: &str| {
        let bytes = assets::encode_lang_catalog(
            [1; 32],
            [2; 32],
            &[assets::LangEntry {
                key: "message.key".into(),
                value: value.into(),
            }],
        )
        .unwrap();
        Arc::new(assets::RuntimeLangCatalog::decode(&bytes).unwrap())
    };
    runtime.set_lang_catalog(language("Base"));
    runtime.set_active_language(Some(language("Local")));
    let input = b"message.key=Server\n";
    runtime.set_server_lang(assets::ServerLangOverlay::read(input.len(), |target| {
        target.copy_from_slice(input);
        true
    }));
    let document =
        protocol::parse_raw_text(r#"{"rawtext":[{"translate":"message.key"}]}"#).unwrap();
    assert_eq!(runtime.resolve_raw_text(&document).text, "Server");
    runtime.set_hide_toasts(true);
    runtime.set_toast_lifetime(123);
    runtime
        .inventory_keys_mut()
        .track_modifier(semantic_input::KeyboardKey::ShiftLeft, true);
    let generation = runtime.item_name_generation();
    let revision = runtime.scoreboards().revision();
    let furnace = Arc::clone(&runtime.furnace_projection);
    let icons = Arc::clone(&runtime.recipe_book_icons);
    let stack = protocol::NetworkItemStack {
        count: 1,
        extra_data: Arc::from([
            255, 255, 1, 10, 0, 0, 9, 4, 0, b'e', b'n', b'c', b'h', 10, 0, 0, 0, 0, 0,
        ]),
        ..protocol::NetworkItemStack::empty()
    };
    let facts = runtime.stack_facts(&stack);
    assert!(facts.has_enchantment_list);
    assert!(!Arc::ptr_eq(&facts, &UiRuntime::new(1).stack_facts(&stack)));
    assert!(runtime.show_older_chat_history());
    assert!(runtime.show_older_chat_history());
    assert_eq!(runtime.chat_editor().as_str(), "first");

    runtime.begin_session(2);

    assert_eq!(runtime.resolve_raw_text(&document).text, "Local");
    assert!(runtime.hides_toasts());
    assert!(runtime.inventory_keys_mut().take_frame().1);
    assert_eq!(runtime.toast_display_millis, 123);
    assert_eq!(runtime.item_name_generation(), generation.wrapping_add(1));
    assert_eq!(runtime.scoreboards().revision(), revision.saturating_add(1));
    assert!(Arc::ptr_eq(&furnace, &runtime.furnace_projection));
    assert!(Arc::ptr_eq(&icons, &runtime.recipe_book_icons));
    assert!(Arc::ptr_eq(&facts, &runtime.stack_facts(&stack)));
    assert!(runtime.show_older_chat_history());
    assert_eq!(runtime.chat_editor().as_str(), "second");
    runtime.set_active_language(None);
    assert_eq!(runtime.resolve_raw_text(&document).text, "Base");
}

#[test]
fn switching_sessions_retires_script_stores_and_keeps_screen_accounting() {
    use protocol::ddui::{
        DataDrivenUiEvent, DataPrimitive, DataStoreChange, DataStoreUpdate, SCRIPT_STORE,
        ScriptedScreen,
    };
    use protocol::test_support::ddui::{custom_form_property, show_events};

    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    for (sequence, event) in show_events(ScriptedScreen::CustomForm, 7, 4, custom_form_property())
        .into_iter()
        .enumerate()
    {
        runtime
            .apply(&mut player, envelope(1, sequence as u64 + 1, event))
            .unwrap();
    }
    let previous = runtime.data_driven_ui().active().unwrap().instance;
    runtime
        .apply(
            &mut player,
            envelope(
                1,
                3,
                UiEvent::DataDrivenUi(DataDrivenUiEvent::Store(vec![DataStoreChange::Update(
                    DataStoreUpdate {
                        store: SCRIPT_STORE.into(),
                        property: "missing".into(),
                        path: "value".into(),
                        value: DataPrimitive::Bool(true),
                        property_update_count: 1,
                        path_update_count: 1,
                    },
                )])),
            ),
        )
        .unwrap();
    runtime
        .apply(
            &mut player,
            envelope(
                1,
                4,
                show_events(ScriptedScreen::CustomForm, 8, 4, custom_form_property())[1].clone(),
            ),
        )
        .unwrap();
    assert_eq!(runtime.data_driven_ui().ignored_store_changes(), 1);

    runtime.begin_session(2);
    player.begin_session(2);

    assert!(runtime.data_driven_ui().active().is_none());
    assert!(
        runtime
            .data_driven_ui()
            .stores()
            .property(SCRIPT_STORE, &ScriptedScreen::CustomForm.property(Some(4)))
            .is_none()
    );
    assert_eq!(runtime.data_driven_ui().ignored_store_changes(), 1);
    assert!(runtime.take_client_packet().is_none());
    for (sequence, event) in show_events(ScriptedScreen::CustomForm, 7, 4, custom_form_property())
        .into_iter()
        .enumerate()
    {
        runtime
            .apply(&mut player, envelope(2, sequence as u64 + 1, event))
            .unwrap();
    }
    assert!(runtime.data_driven_ui().active().unwrap().instance > previous);
}

#[test]
fn switching_sessions_keeps_equipped_emotes_and_retires_playback() {
    let _catalog = cosmetics::test_support::playback_catalog();
    let mut runtime = UiRuntime::new(1);
    runtime.emotes_mut().open();
    runtime.emotes_mut().change_emotes();
    runtime.emotes_mut().activate_slot(0, 100);
    runtime.emotes_mut().activate_slot(2, 100);
    runtime.emotes_mut().activate_slot(2, 100).unwrap();
    let slots = *runtime.emotes().slots();
    let revision = runtime.emotes().playback().unwrap().revision;
    runtime.begin_session(2);
    assert_eq!(runtime.emotes().slots(), &slots);
    assert!(!runtime.emotes().is_open());
    assert!(runtime.emotes().playback().is_none());
    assert!(runtime.emotes_mut().take_preferences().is_some());
    runtime.emotes_mut().open();
    runtime.emotes_mut().activate_slot(2, 0).unwrap();
    assert!(runtime.emotes().playback().unwrap().revision > revision);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn switching_sessions_retires_experience_ingress_and_keeps_host_destination() {
    let mut runtime = UiRuntime::new(1);
    runtime
        .session
        .experiences
        .select_destination("localhost:19132");
    let destination = runtime.session.experiences.audience.clone();
    assert!(destination.is_some());
    runtime.session.experiences.epoch = 12;
    runtime.session.experiences.marker = Some(Arc::from([1u8]));
    runtime.session.experiences.active = true;
    runtime.session.experiences.handled_marker = true;
    runtime
        .session
        .experiences
        .receive(protocol::ExperienceMessage::new(b"{}".to_vec()), 10);
    assert_eq!(runtime.session.experiences.incoming.len(), 1);
    runtime.begin_session(2);
    assert_eq!(runtime.session.experiences.audience, destination);
    assert_eq!(runtime.session.experiences.epoch, 12);
    assert!(!runtime.session.experiences.active);
    assert!(!runtime.session.experiences.handled_marker);
    assert!(!runtime.session.experiences.holds_world_entry());
    assert!(runtime.session.experiences.pop().is_none());
}
