//! Anonymous update notices rendered through the home screen.

use super::{Fixture, GUI_SCALE, MenuView, SIZE, SMALL_SIZE, State};

/// Shows a verified release without contacting the updater or using account data.
pub(super) fn home(writable: bool, protocol_required: bool) -> Fixture {
    let mut view = MenuView::new(true, "Player".into());
    view.updates.ready = Some(launcher::menu::updates::Ready {
        release: update_manifest::build::Build {
            version: next_version(),
            number: 2,
            channel: Some(update_manifest::channel::Channel::Beta),
        },
        switching: false,
        writable,
        protocol_required,
    });
    Fixture::Menu(Box::new(view))
}

/// Demonstrates a newer patch release without copying the current product version.
fn next_version() -> String {
    let core = launcher::PRODUCT_VERSION.split(['-', '+']).next().unwrap();
    let (prefix, patch) = core.rsplit_once('.').expect("product version has a patch");
    format!(
        "{prefix}.{}",
        patch.parse::<u64>().expect("numeric patch") + 1
    )
}

/// Shows a downloaded update waiting for the original client to close.
pub(super) fn pending() -> Fixture {
    let mut view = MenuView::new(true, "Player".into());
    view.updates.active = false;
    view.updates.manual_pending = true;
    Fixture::Menu(Box::new(view))
}

/// Shows the authenticated account placement without loading a real account or skin.
pub(super) fn signed_in() -> Fixture {
    let Fixture::Menu(mut view) = home(true, false) else {
        unreachable!()
    };
    view.auth_state = launcher::menu::auth::AuthState::Authenticated;
    view.feeds.profile.gamertag = "ExamplePlayer".into();
    Fixture::Menu(view)
}

/// Shows optional mod update discovery without app updates or a network request.
pub(super) fn mods(ready: bool, settings: bool) -> Fixture {
    let Fixture::Menu(mut view) = super::mods::declared() else {
        unreachable!()
    };
    if !settings {
        view.screen = launcher::menu::MenuScreen::Home;
    }
    if ready {
        std::sync::Arc::make_mut(&mut view.mods).library.installed[0]
            .metadata
            .update_url = Some(
            "https://github.com/example/zoom-plus/releases/latest/download/zoom-plus.wasm"
                .parse()
                .unwrap(),
        );
        std::sync::Arc::make_mut(&mut view.mods)
            .library
            .updates
            .push(launcher::mods::AvailableUpdate {
                id: 1,
                version: "v1.3.0".into(),
                release_url: "https://github.com/example/zoom-plus/releases/tag/v1.3.0".into(),
            });
    }
    Fixture::Menu(view)
}

/// Shows an explicitly requested in-app update while download or installation is pending.
pub(super) fn mod_installing() -> Fixture {
    let Fixture::Menu(mut view) = mods(true, true) else {
        unreachable!()
    };
    let library = &mut std::sync::Arc::make_mut(&mut view.mods).library;
    library.busy = true;
    library.update_progress = vec![launcher::mods::UpdateProgress {
        id: 1,
        phase: launcher::mods::UpdatePhase::Downloading,
        received_bytes: 512 * 1024,
        total_bytes: 1024 * 1024,
        bytes_per_sec: Some(256 * 1024),
        eta_secs: Some(2),
    }];
    Fixture::Menu(view)
}

/// Shows real post-download stages without fabricating byte percentages for local work.
pub(super) fn mod_stage(phase: launcher::mods::UpdatePhase) -> Fixture {
    let Fixture::Menu(mut view) = mod_installing() else {
        unreachable!()
    };
    let library = &mut std::sync::Arc::make_mut(&mut view.mods).library;
    library.update_progress = vec![launcher::mods::UpdateProgress::stage(1, phase)];
    if phase == launcher::mods::UpdatePhase::Reloading {
        library.installed[0].metadata.version = Some("1.3.0".into());
        library.updates.clear();
        library.active[0].status = launcher::mods::Status::Starting;
    }
    Fixture::Menu(view)
}

/// Shows update status with the selected sidebar tab scrolled into view.
pub(super) fn settings(status: launcher::menu::updates::Status) -> Fixture {
    use launcher::menu::updates::{DownloadProgress, Status};
    use update_manifest::{build::Build, channel::Channel};
    let section = launcher::menu::settings_sections::SettingSection::Updates.definition();
    let mut view = super::settings(section.name);
    view.updates.status = status;
    view.updates.running = Build {
        version: launcher::PRODUCT_VERSION.into(),
        number: 42,
        channel: Some(Channel::Beta),
    };
    if status == Status::Downloading {
        let release = Build {
            version: next_version(),
            number: view.updates.running.number + 1,
            channel: Some(Channel::Nightly),
        };
        std::sync::Arc::make_mut(&mut view.settings_options).set(
            launcher::menu::settings_options::SettingId::UpdateChannel.index(),
            Channel::Nightly.index() as i32,
        );
        view.updates.target = Some(release);
        view.updates.download = Some(DownloadProgress {
            received_bytes: 16 << 20,
            total_bytes: 64 << 20,
        });
    }
    view.focused_action = Some(launcher::menu::MenuAction::SettingsSection(section.index));
    Fixture::Menu(Box::new(view))
}

/// Shows the older Beta choice while the installed client still runs Nightly.
pub(super) fn switch_offer() -> Fixture {
    use launcher::menu::updates::Status;
    use update_manifest::{build::Build, channel::Channel};
    let Fixture::Menu(mut view) = settings(Status::UpToDate) else {
        unreachable!()
    };
    view.updates.running.channel = Some(Channel::Nightly);
    view.updates.target = Some(Build {
        version: view.updates.running.version.clone(),
        number: view.updates.running.number - 1,
        channel: Some(Channel::Beta),
    });
    view.updates.switch_available = true;
    Fixture::Menu(view)
}

/// Shows the completed Beta download with its restart action beside the channel preference.
pub(super) fn switch_ready() -> Fixture {
    let Fixture::Menu(mut view) = switch_offer() else {
        unreachable!()
    };
    view.updates.switch_available = false;
    view.updates.ready = Some(launcher::menu::updates::Ready {
        release: view.updates.target.clone().unwrap(),
        switching: true,
        writable: true,
        protocol_required: false,
    });
    Fixture::Menu(view)
}

/// Home screens with each update notice, at desktop and compact sizes.
pub(super) const STATES: &[State] = &[
    State {
        name: "settings-update-switch",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: switch_offer,
    },
    State {
        name: "settings-update-switch-ready",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: switch_ready,
    },
    State {
        name: "settings-update-switch-small",
        size: SMALL_SIZE,
        gui_scale: 1,
        fixture: switch_offer,
    },
    State {
        name: "home-update-compact",
        size: [480, 640],
        gui_scale: 3,
        fixture: || home(true, false),
    },
    State {
        name: "home-signed-in-update",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: signed_in,
    },
    State {
        name: "home-update-ready",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || home(true, false),
    },
    State {
        name: "home-update-required",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || home(true, true),
    },
    State {
        name: "home-update-download",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || home(false, false),
    },
    State {
        name: "home-update-pending",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: pending,
    },
    State {
        name: "home-update-pending-small",
        size: SMALL_SIZE,
        gui_scale: 1,
        fixture: pending,
    },
    State {
        name: "home-update-small",
        size: SMALL_SIZE,
        gui_scale: 1,
        fixture: || home(true, false),
    },
];
