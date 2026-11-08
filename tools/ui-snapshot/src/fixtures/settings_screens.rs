//! Settings screens at fixed, anonymous states.

use super::*;

/// Global resources while applying and with an empty library, search results, an empty result
/// and the compact settings viewport.
pub(super) const SEARCH_STATES: &[State] = &[
    State {
        name: "settings-global-resources-applying",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || {
            let mut view = settings(SettingSection::GlobalTexturePack.definition().name);
            let packs = Arc::make_mut(&mut view.global_resources);
            packs.busy = true;
            packs.message = oreui::loading_screen::RESOURCE_PACK_WORDS[0].to_owned();
            Fixture::Menu(Box::new(view))
        },
    },
    State {
        name: "settings-global-resources-empty",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || {
            let mut view = settings(SettingSection::GlobalTexturePack.definition().name);
            Arc::make_mut(&mut view.global_resources).available_expanded = true;
            Fixture::Menu(Box::new(view))
        },
    },
    State {
        name: "settings-framerate",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || {
            let mut view = settings(SettingSection::Video.definition().name);
            view.settings_query = "framerate".to_owned();
            Fixture::Menu(Box::new(view))
        },
    },
    State {
        name: "settings-search",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_search("vsync"),
    },
    State {
        name: "settings-search-empty",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_search("no-such-setting"),
    },
    State {
        name: "settings-search-small",
        size: [640, 360],
        gui_scale: GUI_SCALE,
        fixture: || settings_search("vsync"),
    },
    State {
        name: "settings-storage-loading",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || {
            let Fixture::Menu(mut view) = out_of_game::settings_storage() else {
                unreachable!()
            };
            Arc::make_mut(&mut view.storage).loading = true;
            Fixture::Menu(view)
        },
    },
    State {
        name: "settings-storage-search",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || storage_search("harbor"),
    },
    State {
        name: "settings-storage-empty-search",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || storage_search("no matching downloaded pack"),
    },
    State {
        name: "settings-storage-small",
        size: [640, 360],
        gui_scale: GUI_SCALE,
        fixture: out_of_game::settings_storage,
    },
];

/// Settings' Game section while the host plays their own local world, with cheats off and on.
pub(super) const WORLD_STATES: &[State] = &[
    State {
        name: "settings-world-game",
        size: [1280, 3200],
        gui_scale: GUI_SCALE,
        fixture: || settings_world_game(false),
    },
    State {
        name: "settings-world-game-cheats",
        size: [1280, 3200],
        gui_scale: GUI_SCALE,
        fixture: || settings_world_game(true),
    },
    State {
        name: "settings-world-game-live-failed",
        size: [1280, 3200],
        gui_scale: GUI_SCALE,
        fixture: settings_world_game_live_failed,
    },
    State {
        name: "settings-world-game-save-failed",
        size: [1280, 3200],
        gui_scale: GUI_SCALE,
        fixture: settings_world_game_save_failed,
    },
];

/// An unsaved change with its confirmed value restored and a retry message.
fn settings_world_game_save_failed() -> Fixture {
    let Fixture::Menu(mut view) = settings_world_game(false) else {
        unreachable!("world settings are a menu fixture");
    };
    Arc::make_mut(view.world_settings.as_mut().unwrap()).error =
        Some("The local world request failed".into());
    Fixture::Menu(view)
}

/// A saved setting that could not be sent to the running world.
fn settings_world_game_live_failed() -> Fixture {
    let Fixture::Menu(mut view) = settings_world_game(false) else {
        unreachable!("world settings are a menu fixture");
    };
    Arc::make_mut(view.world_settings.as_mut().unwrap()).missed = true;
    Fixture::Menu(view)
}

/// The Game section of an anonymous dedicated-server world.
fn settings_world_game(cheats: bool) -> Fixture {
    use launcher::local_worlds::{in_world, options::WorldOptions, world::World};
    let mut view = settings(SettingSection::Game.definition().name);
    view.over_world = true;
    let mut options = WorldOptions::default();
    options.restore_cheats(cheats);
    let world = World {
        id: "0123456789abcdef".to_owned(),
        name: "My World".to_owned(),
        game_mode: Default::default(),
        generator: Default::default(),
        difficulty: Default::default(),
        backend: bridge::Backend::Bds,
        seed: 1_234_567,
        options,
        options_unavailable: false,
        created_unix: 0,
        last_played_unix: 0,
        size_bytes: 0,
    };
    view.world_settings = in_world::View::of(&world).map(Arc::new);
    Fixture::Menu(Box::new(view))
}

/// Whole settings pages, tall enough to show every row at once.
pub(super) const PAGE_STATES: &[State] = &[
    State {
        name: "settings-video-french-page",
        size: [1280, 2600],
        gui_scale: GUI_SCALE,
        fixture: settings_video_french,
    },
    State {
        name: "settings-accessibility-page",
        size: [1280, 1700],
        gui_scale: GUI_SCALE,
        fixture: || settings_page(SettingSection::Accessibility, false),
    },
    State {
        name: "settings-controller-page",
        size: [1280, 1500],
        gui_scale: GUI_SCALE,
        fixture: || settings_page(SettingSection::ControllerAndSwitch, false),
    },
    State {
        name: "settings-general-page",
        size: [1280, 1500],
        gui_scale: GUI_SCALE,
        fixture: || settings_page(SettingSection::General, false),
    },
    State {
        name: "settings-creator-page",
        size: [1280, 1300],
        gui_scale: GUI_SCALE,
        fixture: || settings_page(SettingSection::Creator, false),
    },
    State {
        name: "settings-account-page",
        size: [1280, 900],
        gui_scale: GUI_SCALE,
        fixture: || settings_page(SettingSection::Account, true),
    },
    State {
        name: "settings-video-page",
        size: [1280, 4600],
        gui_scale: GUI_SCALE,
        fixture: || settings_page(SettingSection::Video, false),
    },
];

/// Shows French video labels and help text with their accented glyphs.
fn settings_video_french() -> Fixture {
    let mut view = settings(SettingSection::Video.definition().name);
    Arc::make_mut(&mut view.settings_options).set_language("fr_FR");
    Fixture::Menu(Box::new(view))
}

/// One settings page, optionally signed in, with the sidebar beside it.
fn settings_page(section: SettingSection, signed_in: bool) -> Fixture {
    let mut view = settings(section.definition().name);
    if signed_in {
        view.auth_state = launcher::menu::auth::AuthState::Authenticated;
    }
    Fixture::Menu(Box::new(view))
}

/// Shows the keyboard options and command macro modifier without a live session.
pub(super) fn settings_keyboard() -> Fixture {
    Fixture::Menu(Box::new(settings("keyboard_and_mouse_forced_index")))
}

/// Scrolls the General section to toggle `id` through the client's focus scrolling; a toggle's
/// action carries the value it switches to.
pub(super) fn settings_general_toggle(id: SettingId) -> Fixture {
    let mut view = settings(SettingSection::General.definition().name);
    view.focused_action = Some(MenuAction::SettingsOption(
        id.index() as u16,
        1 - view.settings_options.value(id),
    ));
    Fixture::Menu(Box::new(view))
}

/// The Advanced rows with Verbose logging and Record network session turned on.
pub(super) fn settings_general_diagnostics_on() -> Fixture {
    let mut view = settings(SettingSection::General.definition().name);
    let mut options = (*view.settings_options).clone();
    for id in [SettingId::VerboseLogging, SettingId::RecordNetworkSession] {
        options.set(id.index(), 1);
    }
    view.settings_options = Arc::new(options);
    view.focused_action = Some(MenuAction::SettingsOption(
        SettingId::RecordNetworkSession.index() as u16,
        0,
    ));
    Fixture::Menu(Box::new(view))
}

/// Shows the same physical hotbar binding with a fallback or an observed AZERTY label.
pub(super) fn settings_keyboard_layout(observed: bool) -> Fixture {
    use launcher::menu::settings_options::{
        BindingId, KEY_BINDINGS, control_bindings::decode_control,
    };

    let mut view = settings("keyboard_and_mouse_forced_index");
    let row = KEY_BINDINGS
        .iter()
        .position(|(_, name)| *name == BindingId::Hotbar1.name())
        .unwrap();
    let settings = Arc::make_mut(&mut view.settings_options);
    assert!(settings.remap(row, decode_control(0x14).unwrap()));
    if observed {
        settings.set_key_label(0x14, "a");
    }
    Fixture::Menu(Box::new(view))
}

/// Shows editable matches from outside the selected settings category.
fn settings_search(query: &str) -> Fixture {
    let mut view = settings("accessibility_forced_index");
    view.settings_query = query.to_owned();
    view.field = Some(launcher::menu::MenuField::SettingsQuery);
    view.focused_action = Some(MenuAction::SearchSettings);
    view.caret.byte = query.len();
    Fixture::Menu(Box::new(view))
}

/// Shows the Accessibility GUI-scale slider with a 1440p window's three modifiers, set to the
/// middle one.
pub(super) fn settings_gui_scale() -> Fixture {
    let mut view = settings("accessibility_forced_index");
    view.gui_scale_choices = ui::DesktopGuiScale::for_window([2560, 1440])
        .choices()
        .collect();
    view.gui_scale_offset = view.gui_scale_choices[1].offset;
    Fixture::Menu(Box::new(view))
}

/// Focuses the occlusion toggle at its saved value in the Video settings viewport.
pub(super) fn settings_video_culling(enabled: bool) -> Fixture {
    let mut view = settings("video_forced_index");
    Arc::make_mut(&mut view.settings_options)
        .set(SettingId::ChunkOcclusionCulling.index(), i32::from(enabled));
    view.focused_action = Some(MenuAction::SettingsOption(
        SettingId::ChunkOcclusionCulling.index() as u16,
        i32::from(!enabled),
    ));
    Fixture::Menu(Box::new(view))
}

/// Filters an anonymous cache inventory without changing its original row indices.
fn storage_search(query: &str) -> Fixture {
    let Fixture::Menu(mut view) = out_of_game::settings_storage() else {
        unreachable!()
    };
    view.storage_query = query.into();
    Fixture::Menu(view)
}
