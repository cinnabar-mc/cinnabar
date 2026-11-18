//! Tests the app chat input system against the shared UI renderer.

use json_ui::{Draw, DrawNode};

use super::*;

use client_ui::test_support::{engine_presentation, engine_presentation_with};

#[test]
fn link_confirmation_consumes_typing_and_escape_preserves_chat_draft() {
    use bevy::{
        input::keyboard::{Key, KeyboardInput},
        prelude::*,
        window::{CursorOptions, PrimaryWindow},
    };
    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping link_confirmation input: missing installed UI carrier (make assets)");
        return;
    };
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = UiRuntime::new(1);
    chat(
        &mut player,
        &mut runtime,
        1,
        "Visit https://example.com/info",
    );
    runtime.open_chat(&mut player);
    runtime.insert_chat_text("unsent draft").unwrap();
    build(&player, &mut presentation, &runtime, 0);
    let index = presentation
        .chat_hits()
        .iter()
        .find_map(|(hit, _)| match hit {
            ui_screens::chat_screen::ChatHit::Link(index) => Some(*index),
            _ => None,
        })
        .expect("visible chat URL has a hit target");
    presentation.request_chat_link(index);
    assert!(presentation.chat_link_confirmation_open());
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<bevy::input::mouse::AccumulatedMouseMotion>()
        .insert_resource(player)
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(UiPresentationResource(presentation))
        .add_systems(Update, crate::ui_runtime::drive_chat_keyboard_input);
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    for key in [KeyCode::KeyX, KeyCode::Enter, KeyCode::Escape] {
        app.world_mut().write_message(KeyboardInput {
            key_code: key,
            logical_key: Key::Character("x".into()),
            state: bevy::input::ButtonState::Pressed,
            text: Some("x".into()),
            repeat: false,
            window,
        });
        app.update();
        let runtime = app.world().resource::<UiRuntimeResource>();
        assert!(runtime.chat_focused());
        assert_eq!(runtime.chat_editor().as_str(), "unsent draft");
        assert!(runtime.pending_chat_sends().is_empty());
        assert_eq!(
            app.world()
                .resource::<UiPresentationResource>()
                .chat_link_confirmation_open(),
            key != KeyCode::Escape
        );
    }
}

#[test]
fn chat_settings_retains_the_editor_and_completion_when_returning() {
    use bevy::{
        input::keyboard::{Key, KeyboardInput},
        prelude::*,
        window::{CursorOptions, PrimaryWindow},
    };
    use client_shell::menu::MenuRuntime;
    use launcher::menu::{
        MenuAction, MenuScreen,
        settings_options::{SettingId, chat::CHAT_INPUTS},
        settings_sections::SettingSection,
    };
    use ui_screens::chat_screen::ChatHit;

    for (input, style) in CHAT_INPUTS.iter().enumerate() {
        let Some(mut presentation) = engine_presentation() else {
            eprintln!(
                "skipping chat_settings_retains_the_editor_and_completion_when_returning: missing installed UI carrier (make assets)"
            );
            return;
        };
        let mut menu = MenuRuntime::new(false, 2, "test".into());
        menu.activate(MenuAction::SettingsOption(
            SettingId::CinnabarChatInput.index() as u16,
            input as i32,
        ));
        presentation.set_chat_settings_snapshot(menu.settings_snapshot());
        let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
        let mut runtime = UiRuntime::new(1);
        runtime.open_chat(&mut player);
        runtime.insert_chat_text("/tell Alex hello 🌍").unwrap();
        runtime.mutate_chat_editor(ui::ChatEditor::select_left);
        let editor = runtime.chat_editor().clone();
        build(&player, &mut presentation, &runtime, 0);
        let bounds = presentation
            .chat_hits()
            .into_iter()
            .find_map(|(hit, bounds)| (hit == ChatHit::SettingsOpen).then_some(bounds))
            .unwrap_or_else(|| panic!("{} chat has a settings button", style.label));
        let mut window = Window {
            focused: true,
            resolution: (1280, 720).into(),
            ..Default::default()
        };
        window.set_cursor_position(Some(Vec2::new(
            (bounds.min().x() + bounds.max().x()) * 0.5,
            (bounds.min().y() + bounds.max().y()) * 0.5,
        )));
        let mut app = App::new();
        app.add_message::<KeyboardInput>()
            .init_resource::<Time<Real>>()
            .init_resource::<client_presentation::local_player::LocalPlayerFrameCarrier>()
            .init_resource::<client_presentation::local_player::InteractionOriginSnapshot>()
            .init_resource::<client_state::SemanticInputSnapshot>()
            .init_resource::<crate::runtime::world::ClientWorld>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<bevy::input::mouse::AccumulatedMouseMotion>()
            .init_resource::<Touches>()
            .insert_resource(player)
            .insert_resource(UiRuntimeResource(runtime))
            .insert_resource(UiPresentationResource(presentation))
            .insert_resource(menu)
            .add_systems(
                Update,
                (
                    crate::ui_runtime::drive_chat_ui_actions,
                    crate::ui_runtime::drive_chat_keyboard_input,
                )
                    .chain(),
            );
        let window = app
            .world_mut()
            .spawn((window, CursorOptions::default(), PrimaryWindow))
            .id();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset_all();
        let menu = app.world().resource::<MenuRuntime>();
        assert!(menu.is_visible());
        assert_eq!(menu.view().screen, MenuScreen::Settings);
        assert_eq!(menu.settings_section(), Some(SettingSection::Chat));
        assert_eq!(
            app.world().resource::<UiRuntimeResource>().chat_editor(),
            &editor,
            "{}",
            style.label
        );

        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyX,
            logical_key: Key::Character("x".into()),
            state: bevy::input::ButtonState::Pressed,
            text: Some("x".into()),
            repeat: false,
            window,
        });
        app.update();
        {
            let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
            menu.activate(MenuAction::SettingsOption(
                SettingId::HideChat.index() as u16,
                1,
            ));
            menu.activate(MenuAction::AddBack);
        }
        app.update();
        assert!(!app.world().resource::<MenuRuntime>().is_visible());
        let mut runtime = app.world_mut().resource_mut::<UiRuntimeResource>();
        assert!(runtime.chat_focused());
        assert_eq!(runtime.chat_editor(), &editor);
        assert!(runtime.pending_chat_sends().is_empty());
        let completion = runtime.take_chat_autocomplete_request().unwrap();
        assert_eq!(completion.input.as_ref(), editor.as_str());
        assert_eq!(usize::from(completion.cursor_byte), editor.cursor_byte());
    }
}

/// Finds one rendered label by its content.
fn text_node<'a>(nodes: &'a [DrawNode], wanted: &str) -> Option<&'a DrawNode> {
    nodes
        .iter()
        .find(|node| matches!(&node.draw, Draw::Text { text, .. } if text == wanted))
}

/// Applies one authoritative chat message.
fn chat(
    player_runtime: &mut client_state::PlayerRuntime,
    runtime: &mut UiRuntime,
    sequence: u64,
    message: &str,
) {
    runtime
        .apply(
            player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: sequence,
                local_millis: 0,
                server_tick: None,
                event: chat_event(message),
            },
        )
        .unwrap();
}

/// Lays out one gameplay frame.
fn build(
    player_runtime: &client_state::PlayerRuntime,
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    now: u64,
) {
    presentation
        .build(
            player_runtime,
            runtime,
            now,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
}

#[test]
fn wheel_input_system_scrolls_the_open_chat() {
    let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

    use bevy::{
        input::mouse::AccumulatedMouseScroll, prelude::*, time::Real, window::PrimaryWindow,
    };
    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping wheel_input_system_scrolls_the_open_chat: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    for sequence in 1..=80 {
        chat(
            &mut player_runtime,
            &mut runtime,
            sequence,
            &format!("line {sequence}"),
        );
    }
    runtime.open_chat(&mut player_runtime);
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(text_node(presentation.chat_draw_nodes(), "line 1").is_none());
    let mut app = App::new();
    app.init_resource::<Time<Real>>()
        .init_resource::<client_presentation::local_player::LocalPlayerFrameCarrier>()
        .init_resource::<client_presentation::local_player::InteractionOriginSnapshot>()
        .init_resource::<client_state::SemanticInputSnapshot>()
        .init_resource::<crate::runtime::world::ClientWorld>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .insert_resource(AccumulatedMouseScroll {
            delta: Vec2::new(0.0, 1000.0),
            ..Default::default()
        })
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(player_runtime)
        .insert_resource(UiPresentationResource(presentation))
        .add_systems(Update, crate::ui_runtime::drive_chat_ui_actions);
    app.world_mut().spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));
    app.update();
    let player_runtime = app
        .world_mut()
        .remove_resource::<client_state::PlayerRuntime>()
        .unwrap();
    let runtime = app
        .world_mut()
        .remove_resource::<UiRuntimeResource>()
        .unwrap();
    let mut presentation = app
        .world_mut()
        .remove_resource::<UiPresentationResource>()
        .unwrap();
    build(&player_runtime, &mut presentation, &runtime, 0);
    let oldest = text_node(presentation.chat_draw_nodes(), "line 1").unwrap();
    assert!(oldest.dest.y + oldest.dest.h > oldest.clip.y);
    assert!(oldest.dest.y < oldest.clip.y + oldest.clip.h);
}

/// Collects visible labels from the resolved chat screen.
fn texts(nodes: &[DrawNode]) -> Vec<&str> {
    nodes
        .iter()
        .filter(|node| node.alpha > 0.0)
        .filter_map(|node| match &node.draw {
            Draw::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn server_chat_screen_keeps_the_java_layout_and_input_policy() {
    let mut player_runtime = client_state::PlayerRuntime(player_state::PlayerState::new(1));

    let Some(mut presentation) =
        engine_presentation_with(client_ui::test_support::pack_harness::font())
    else {
        eprintln!(
            "skipping server_chat_screen_keeps_the_java_layout_and_input_policy: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player_runtime);
    presentation.set_server_ui_pack(&client_ui::ui_runtime::presentation::forms::ServerUiPack {
        ui_layers: vec![vec![(
            "ui/chat_screen.json".into(),
            br#"{"namespace":"chat","chat_screen":{"render_game_behind":false},
                "chat_screen_content":{"controls":[{"server_marker":{"type":"label",
                    "text":"Server chat","offset":[0,40]}}]}}"#
                .to_vec(),
        )]],
        ..Default::default()
    });
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(!texts(presentation.chat_draw_nodes()).contains(&"Server chat"));
    let menu = client_shell::menu::MenuRuntime::new(false, 2, "Tester".into());
    assert!(presentation.renders_game_behind(&player_runtime, &runtime, Some(&menu)));
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    client_ui::ui_runtime::presentation::forms::snapshot::write(&input, "server-chat");
    presentation.set_server_ui_pack(&Default::default());
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(!texts(presentation.chat_draw_nodes()).contains(&"Server chat"));
    assert!(presentation.renders_game_behind(&player_runtime, &runtime, Some(&menu)));
    assert!(presentation.absorbs_gameplay_input(&player_runtime, &runtime, &menu));
}
