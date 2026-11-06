//! Anonymous chat screens for message, background and completion captures.

use super::*;

/// Exercises the native chat collection with an unfinished mention.
pub(super) fn mentions() -> Fixture {
    Fixture::Chat {
        server_hover: None,
        receive_while_scrolled: false,
        draft: "@",
        suggestions: ["@here", "@Alex", "@Steve"].map(Arc::from).into(),
        messages: Default::default(),
        options: Default::default(),
    }
}

/// Exercises the native command completion collection with target candidates.
pub(super) fn command_targets() -> Fixture {
    Fixture::Chat {
        server_hover: None,
        receive_while_scrolled: false,
        draft: "/give @",
        suggestions: ["@a", "@e", "@p", "@r", "@s"].map(Arc::from).into(),
        messages: Default::default(),
        options: Default::default(),
    }
}

/// Shows readable history and a draft under a selected background-opacity setting.
pub(super) fn background(percent: i32) -> Fixture {
    use launcher::menu::settings_options::{SettingId, SettingsOptions};
    let mut options = SettingsOptions::default();
    options.set(SettingId::ChatBackgroundOpacity.index(), percent);
    let messages = [
        format!("Chat background opacity: {percent}%"),
        "The chat text stays visible.".into(),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, message)| ui::ChatMessage {
        fifo_sequence: index as u64 + 1,
        received_millis: 0,
        kind: ui::ChatMessageKind::System,
        source: None,
        message: message.into(),
        parameters: Arc::from([]),
    })
    .collect();
    Fixture::Chat {
        server_hover: None,
        receive_while_scrolled: false,
        draft: "Typing a message",
        suggestions: Vec::new(),
        messages,
        options: Arc::new(options),
    }
}

/// Shows the command input and suggestions with chat backgrounds set to zero percent.
pub(super) fn command_background() -> Fixture {
    completion("/", &["/give", "/help", "/say"])
}

/// Shows equal-width history backgrounds and a message that wraps inside them.
pub(super) fn history_width() -> Fixture {
    let mut fixture = background(50);
    if let Fixture::Chat { messages, .. } = &mut fixture {
        messages.push_back(ui::ChatMessage {
            fifo_sequence: messages.len() as u64 + 1,
            received_millis: 0,
            kind: ui::ChatMessageKind::System,
            source: None,
            message: "A longer chat message wraps inside the history box while every visible line keeps the same background width.".into(),
            parameters: Arc::from([]),
        });
    }
    fixture
}

/// Shows history, a draft and optional completions under one Chat Position and Chat Input pair.
pub(super) fn input(
    top: bool,
    bedrock: bool,
    draft: &'static str,
    suggestions: &[&str],
) -> Fixture {
    use launcher::menu::settings_options::{CHAT_INPUT_OPTION, CHAT_POSITION_OPTION};

    let mut fixture = history_width();
    if let Fixture::Chat {
        options,
        draft: shown,
        suggestions: shown_suggestions,
        ..
    } = &mut fixture
    {
        let options = Arc::make_mut(options);
        options.set(CHAT_POSITION_OPTION.id.index(), i32::from(top));
        options.set(CHAT_INPUT_OPTION.id.index(), i32::from(bedrock));
        *shown = draft;
        *shown_suggestions = suggestions.iter().map(|text| Arc::from(*text)).collect();
    }
    fixture
}

/// Shows completions after an anonymous draft with transparent message backgrounds.
pub(super) fn completion(draft: &'static str, suggestions: &[&str]) -> Fixture {
    use launcher::menu::settings_options::SettingsOptions;

    let mut options = SettingsOptions::default();
    options.set(SettingId::ChatBackgroundOpacity.index(), 0);
    Fixture::Chat {
        server_hover: None,
        receive_while_scrolled: false,
        draft,
        suggestions: suggestions.iter().map(|text| Arc::from(*text)).collect(),
        messages: Default::default(),
        options: Arc::new(options),
    }
}

/// Keeps encoded message metadata out of history and reveals it through a hover renderer.
#[cfg(feature = "test-support")]
pub(super) fn server_content(hover: bool) -> Fixture {
    Fixture::Chat {
        draft: "",
        suggestions: Vec::new(),
        messages: [ui::ChatMessage {
            fifo_sequence: 1,
            received_millis: 0,
            kind: ui::ChatMessageKind::System,
            source: None,
            message: ui_screens::test_support::hover_chat::message().into(),
            parameters: Arc::from([]),
        }]
        .into(),
        options: Default::default(),
        server_hover: Some(hover),
        receive_while_scrolled: false,
    }
}

/// Receive chat while the reader is partway through older history.
pub(super) fn incoming_while_scrolled() -> Fixture {
    let mut fixture = background(50);
    if let Fixture::Chat {
        messages,
        draft,
        receive_while_scrolled,
        ..
    } = &mut fixture
    {
        *draft = "Reading older messages";
        *receive_while_scrolled = true;
        messages.clear();
        for index in 1..=80 {
            messages.push_back(ui::ChatMessage {
                fifo_sequence: index,
                received_millis: 0,
                kind: ui::ChatMessageKind::System,
                source: None,
                message: format!("Older message {index}: continuing the conversation").into(),
                parameters: Arc::from([]),
            });
        }
    }
    fixture
}

/// Shows white mentions, surrounding styles and explicit white under green chat.
pub(super) fn colored_mentions() -> Fixture {
    let mut fixture = background(50);
    if let Fixture::Chat {
        messages, options, ..
    } = &mut fixture
    {
        let settings = Arc::make_mut(options);
        settings.set(
            launcher::menu::settings_options::SettingId::ChatColor.index(),
            1,
        );
        settings.set(
            launcher::menu::settings_options::SettingId::MentionsColor.index(),
            0,
        );
        *messages = [
            "Local player: Alex",
            "Hello @Alex back to green",
            "Unrelated @Steve and @something stay green",
            "Longer @Alex123 stays green; @here reaches everyone",
            "§c§l§oAlert @Alex still red and styled",
            "Green §fexplicit white§r green again",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, text)| ui::ChatMessage {
            fifo_sequence: index as u64 + 1,
            received_millis: 0,
            kind: ui::ChatMessageKind::System,
            source: None,
            message: text.into(),
            parameters: Arc::from([]),
        })
        .collect();
    }
    fixture
}
