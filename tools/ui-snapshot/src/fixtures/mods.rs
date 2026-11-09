//! A Mods tab library with and without self-declared component metadata.

use super::{Arc, Fixture, GUI_SCALE, State, TALL_SIZE, settings};
use launcher::menu::auth::AuthState;
use launcher::mods::{ActiveMod, InstalledMod, Library, Permission, Snapshot, Status};
use ui::mod_metadata::ModMetadata;

/// Settings → Mods captures.
pub(super) const STATES: &[State] = &[
    State {
        name: "home-marketplace",
        size: super::SIZE,
        gui_scale: GUI_SCALE,
        fixture: home_marketplace,
    },
    State {
        name: "home-marketplace-small",
        size: super::SMALL_SIZE,
        gui_scale: 1,
        fixture: home_marketplace,
    },
    State {
        name: "settings-mods-marketplace-empty",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: marketplace_empty,
    },
    State {
        name: "settings-mods-marketplace-empty-narrow",
        size: [480, 960],
        gui_scale: GUI_SCALE,
        fixture: marketplace_empty,
    },
    State {
        name: "settings-mods-marketplace-signed-in",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: marketplace_signed_in,
    },
    State {
        name: "settings-mods-marketplace-downloaded",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: marketplace_downloaded,
    },
    State {
        name: "settings-mods-marketplace-withdrawn",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: marketplace_withdrawn,
    },
    State {
        name: "settings-mods-marketplace-customize",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: marketplace_customize,
    },
    State {
        name: "settings-mods",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: declared,
    },
    State {
        name: "settings-mods-compact-library",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: compact_library,
    },
    State {
        name: "settings-mods-undeclared",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: undeclared,
    },
    State {
        name: "settings-mods-permissions-collapsed",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: collapsed,
    },
    State {
        name: "settings-mods-permissions-consent",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: consent,
    },
    State {
        name: "settings-mods-permissions-requested",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: requested_only,
    },
    State {
        name: "settings-mods-permissions-all",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: show_all,
    },
    State {
        name: "settings-mods-permissions-none",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: no_grants,
    },
    State {
        name: "settings-mods-permissions-none-narrow",
        size: [360, 900],
        gui_scale: GUI_SCALE,
        fixture: no_grants,
    },
    State {
        name: "settings-mods-permissions-narrow",
        size: [360, 900],
        gui_scale: GUI_SCALE,
        fixture: show_all,
    },
    State {
        name: "settings-mods-permissions-undeclared-consent",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: undeclared_permissions,
    },
];

/// A signed-out home with no service fixtures.
fn home_marketplace() -> Fixture {
    Fixture::Menu(Box::new(launcher::menu::MenuView::new(
        true,
        "Player".into(),
    )))
}

/// The Mods page over an empty local library in the given account state.
fn marketplace_view(auth_state: AuthState) -> Box<launcher::menu::MenuView> {
    let mut view = settings(launcher::mods::SECTION);
    view.auth_state = auth_state;
    view.mods = Arc::new(Snapshot::default());
    Box::new(view)
}

/// An empty local library before the player connects an account.
fn marketplace_empty() -> Fixture {
    Fixture::Menu(marketplace_view(AuthState::SignedOut))
}

/// The same empty library with a signed-in account; no successful sync is implied.
fn marketplace_signed_in() -> Fixture {
    Fixture::Menu(marketplace_view(AuthState::Authenticated))
}

/// One expanded, verified website selection; `status` is `None` until the player enables it.
fn marketplace_library(status: Option<Status>, message: String) -> Fixture {
    let mut view = marketplace_view(AuthState::Authenticated);
    view.mods = Arc::new(Snapshot {
        library: Library {
            installed: vec![InstalledMod {
                id: 1,
                file: format!(
                    "{}fixture-1f2e3d.wasm",
                    launcher::mods::MARKETPLACE_FILE_PREFIX
                ),
                bytes: 96_256,
                permissions: vec![Permission {
                    applicable: true,
                    name: "time override".into(),
                    description: "Lets the mod fix the local time of day.".into(),
                    enabled: false,
                    requested: Some(true),
                    requested_allowed: true,
                    sensitive: false,
                }],
                metadata: ModMetadata {
                    name: Some("Daylight Lock".into()),
                    description: Some("Keeps your local sky at a chosen time of day.".into()),
                    version: Some("1.0.0".into()),
                    authors: Some("Example Author".into()),
                    icon: Some(icon()),
                    ..ModMetadata::default()
                },
            }],
            active: status
                .map(|status| ActiveMod { id: 1, status })
                .into_iter()
                .collect(),
            message,
            ..Library::default()
        },
        details_expanded: Some(1),
        ..Snapshot::default()
    });
    Fixture::Menu(view)
}

/// A verified download waiting, disabled, in My mods.
fn marketplace_downloaded() -> Fixture {
    marketplace_library(None, launcher::mods::MARKETPLACE_ADDED.into())
}

/// An enabled marketplace mod held after staff withdrew its release.
fn marketplace_withdrawn() -> Fixture {
    marketplace_library(
        Some(Status::Failed(launcher::mods::marketplace_withdrawn(
            "Requests more permissions than its listing describes.",
        ))),
        String::new(),
    )
}

/// A downloaded marketplace mod with Customize open, offering only its requested permissions.
fn marketplace_customize() -> Fixture {
    let Fixture::Menu(mut view) = marketplace_library(None, String::new()) else {
        unreachable!("the marketplace fixture is a menu")
    };
    Arc::make_mut(&mut view.mods).customizing = Some(1);
    Fixture::Menu(view)
}

/// Three components; `metadata` decides whether they describe themselves.
fn view(metadata: bool) -> Fixture {
    let card = |id, file: &str, declared: ModMetadata| InstalledMod {
        id,
        file: file.into(),
        bytes: 182_272,
        permissions: vec![Permission {
            applicable: true,
            name: "time override".into(),
            description: "Lets the mod fix the local time of day.".into(),
            enabled: true,
            requested: Some(true),
            requested_allowed: true,
            sensitive: false,
        }],
        metadata: if metadata {
            declared
        } else {
            ModMetadata::default()
        },
    };
    let mut view = settings(launcher::mods::SECTION);
    view.mods = Arc::new(Snapshot {
        library: Library {
            installed: vec![
                card(
                    1,
                    "zoom_plus-1.2.0.wasm",
                    ModMetadata {
                        name: Some("Zoom Plus".into()),
                        description: Some(
                            "Smooth, scroll-adjustable zoom on a key of your choice.".into(),
                        ),
                        version: Some("1.2.0".into()),
                        authors: Some("Example Author".into()),
                        licenses: Some("MIT OR Apache-2.0".into()),
                        icon: Some(icon()),
                        update_repository: None,
                        update_url: None,
                        revision_update: None,
                    },
                ),
                card(
                    2,
                    "minimap.wasm",
                    ModMetadata {
                        name: Some("Minimap".into()),
                        version: Some("0.4.1".into()),
                        ..ModMetadata::default()
                    },
                ),
                card(3, "fps-overlay.wasm", ModMetadata::default()),
            ],
            active: vec![ActiveMod {
                id: 1,
                status: Status::Running,
            }],
            ..Library::default()
        },
        details_expanded: Some(1),
        ..Snapshot::default()
    });
    Fixture::Menu(Box::new(view))
}

/// Cards named by their declared metadata, with one embedded icon.
pub fn declared() -> Fixture {
    view(true)
}

/// The same files without metadata: file names and placeholder icons.
pub fn undeclared() -> Fixture {
    view(false)
}

/// Original procedural art: a lens ring on a diagonal gradient.
pub(super) fn icon() -> Arc<[u8]> {
    const SIDE: u32 = 64;
    let image = image::RgbaImage::from_fn(SIDE, SIDE, |x, y| {
        let (dx, dy) = (x as f32 - 27.0, y as f32 - 27.0);
        let distance = (dx * dx + dy * dy).sqrt();
        let handle = x > 38 && y > 38 && x.abs_diff(y) < 5 && x < 58;
        if (14.0..19.0).contains(&distance) || handle {
            image::Rgba([245, 245, 240, 255])
        } else {
            let t = (x + y) as f32 / (2 * SIDE) as f32;
            image::Rgba([
                (200.0 - 120.0 * t) as u8,
                (60.0 + 40.0 * t) as u8,
                (40.0 + 150.0 * t) as u8,
                255,
            ])
        }
    });
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("encode fixture icon");
    png.into()
}

/// A sample library for permission summaries, consent and both customization modes.
fn permissions(customizing: bool, show_all: bool, consent: bool, declared: bool) -> Fixture {
    let Fixture::Menu(mut view) = view(true) else {
        unreachable!()
    };
    let snapshot = Arc::make_mut(&mut view.mods);
    snapshot.library.installed.truncate(1);
    snapshot.details_expanded = Some(1);
    snapshot.customizing = customizing.then_some(1);
    snapshot.show_all_permissions = show_all;
    snapshot.consent = consent.then_some(1);
    snapshot.library.installed[0].permissions = [
        (
            "HUD",
            "Allows bounded HUD cards and a custom crosshair.",
            false,
            false,
        ),
        (
            "Camera rotation",
            "Allows bounded camera rotation and camera rigs.",
            true,
            true,
        ),
        (
            "Controls",
            "Allows key bindings and the mod settings panel.",
            true,
            true,
        ),
        (
            "Settings file",
            "Allows this mod's companion settings file.",
            true,
            false,
        ),
        (
            "Item use",
            "Allows air use after slot changes and while attacking.",
            false,
            true,
        ),
        (
            "Attack reach",
            "Allows bounded attack range and held attack requests.",
            false,
            true,
        ),
        (
            "Packet delay",
            "Allows bounded post-login packet delay.",
            false,
            true,
        ),
        (
            "Movement",
            "Allows physics snapshots and one-shot jumps.",
            false,
            true,
        ),
        (
            "Session packets",
            "Allows session packet access for packet components.",
            false,
            true,
        ),
        (
            "WASI runtime",
            "Allows the packet runtime's WASI services.",
            false,
            false,
        ),
    ]
    .into_iter()
    .map(|(name, description, requested, sensitive)| Permission {
        applicable: !matches!(name, "Session packets" | "WASI runtime"),
        name: name.into(),
        description: description.into(),
        enabled: requested && !consent,
        requested: declared.then_some(requested),
        requested_allowed: requested && !consent,
        sensitive,
    })
    .collect();
    if customizing {
        view.focused_action = Some(launcher::menu::MenuAction::Mods(
            launcher::mods::Action::Permission(1, 1, false),
        ));
        view.navigation_focus_visible = true;
    }
    Fixture::Menu(view)
}

/// An expanded card with a short grant summary and its customization entry.
pub fn collapsed() -> Fixture {
    permissions(false, false, false, true)
}

/// A first-enable card waiting for explicit requested-grant consent.
pub fn consent() -> Fixture {
    permissions(false, false, true, true)
}

/// A compact grid limited to this sample mod's three requests.
pub fn requested_only() -> Fixture {
    permissions(true, false, false, true)
}

/// Requested permissions precede the optional permissions in the expanded grid.
pub fn show_all() -> Fixture {
    permissions(true, true, false, true)
}

/// An undeclared component with every permission off and the full grid open.
pub fn no_grants() -> Fixture {
    permissions(true, true, true, false)
}

/// A component without a declaration can only customize its grants.
pub fn undeclared_permissions() -> Fixture {
    permissions(false, false, true, false)
}

/// Three closed cards with long descriptions to compare list density.
pub fn compact_library() -> Fixture {
    let Fixture::Menu(mut view) = view(true) else {
        unreachable!()
    };
    let snapshot = Arc::make_mut(&mut view.mods);
    snapshot.details_expanded = None;
    for item in &mut snapshot.library.installed {
        item.metadata.description = Some(format!(
            "{} adds customizable features to your game. Choose your preferred controls, adjust the settings to suit your play style, and use the permissions panel to decide which capabilities it can access.",
            item.name()
        ));
        item.metadata.authors = Some("Example Author".into());
    }
    Fixture::Menu(view)
}
