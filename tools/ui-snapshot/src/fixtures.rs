//! Named, anonymous screen fixtures. Add a state here when a PR needs a new capture.

mod armor_trims;
mod audio_output;
mod chat;
mod collection_reveal;
#[cfg(feature = "test-support")]
mod cosmetics;
mod graphics_recovery;
mod hud_hotbar;
pub(crate) mod hud_surface;
mod local_identity;
mod minecraft_import;
mod mods;
mod out_of_game;
mod registry;
mod roster;
mod server_forms;
mod settings_screens;
pub use settings_screens::settings;
mod shader_options;
mod shader_pack;
mod social_settings;
mod touch_editor;
#[cfg(feature = "test-support")]
pub(crate) use touch_editor::TouchUtilityView;
mod updates;

use std::sync::Arc;

use launcher::menu::{
    MenuAction, MenuScreen, MenuView, settings_options::SettingId,
    settings_sections::SettingSection,
};
use ui_screens::player_list::RosterRow;

/// A repeatable viewport and the runtime observations its screen paints.
pub struct State {
    pub name: &'static str,
    pub size: [u32; 2],
    pub gui_scale: u8,
    pub fixture: fn() -> Fixture,
}

/// Inputs borrowed by the same screen painters the client uses.
pub enum Fixture {
    /// Worn and inventory armor with and without trims.
    #[cfg(feature = "test-support")]
    ArmorTrims {
        trimmed: bool,
    },
    /// Unequal mask edges and high-resolution sprite minification.
    MaskRegression,
    /// Opaque black-backed cursor art on contrasting backgrounds.
    CrosshairCoverage {
        fixed: bool,
    },
    /// Original sprite samples before and after alpha-only outline shading.
    SpriteOutline {
        outline: bool,
    },
    /// Adjacent sparse saturation masks before and after overlap-aware exclusion.
    SaturationOverlap {
        crop: bool,
        source_alpha: u8,
    },
    /// A label whose unavailable primary face selects its installed backup.
    BackupFont,
    /// A primary click with a held stack over the installed creative catalog's empty viewport.
    CreativeCatalogDelete,
    /// An original checker repeated beyond a small sprite's former draw cutoff.
    TiledImage {
        scale: f64,
    },
    /// A server form whose editable controls are locked during a session.
    #[cfg(feature = "test-support")]
    ControlLocked,
    /// An equipped original cosmetic rendered by the full production particle-preview path.
    #[cfg(feature = "test-support")]
    CosmeticParticles {
        view: client_ui::ui_runtime::presentation::player_preview::PreviewView,
        emitting: bool,
    },
    Menu(Box<MenuView>),
    /// Built-in touch editor interactions through production rendering and pointer routing.
    #[cfg(feature = "test-support")]
    TouchEditor {
        phase: u8,
    },
    /// Gameplay touch controls and their utility overlays.
    #[cfg(feature = "test-support")]
    TouchUtilities(TouchUtilityView),
    /// The survival HUD's hotbar flush with the bottom edge or at vanilla Bedrock's height.
    #[cfg(feature = "test-support")]
    BedrockHotbar {
        raised: bool,
    },
    Loading(ui::LoadingStage),
    /// The client-owned cover shown while a resource-pack snapshot is published.
    ResourcePackLoading,
    Chat {
        draft: &'static str,
        suggestions: Vec<Arc<str>>,
        messages: std::collections::VecDeque<ui::ChatMessage>,
        options: Arc<launcher::menu::settings_options::SettingsOptions>,
        server_hover: Option<bool>,
        /// Scroll into history, then receive a wrapped message before the final capture.
        receive_while_scrolled: bool,
    },
    /// The held tab roster's rows.
    TabList(Vec<RosterRow>, &'static str),
    /// Two original custom-form inputs after selection moves from the first to the second.
    EditSelection,
    /// An externally supplied bounded mod panel, the same mod's HUD surface it may embed
    /// controls of, and the local player's portrait.
    ModPanel(
        Box<ui::mod_panel::Panel>,
        Option<Box<ui::mod_panel::Surface>>,
        Option<(&'static str, render_model::UiTexturePage)>,
    ),
    /// A mod's HUD surface over gameplay, projected through a forward-looking camera.
    HudSurface(Box<ui::mod_panel::Surface>),
    /// Two same-ID requests arriving while the first menu is still open.
    #[cfg(feature = "test-support")]
    FormOverlap,
    /// An answered form after immediate retirement or during its visual handoff.
    #[cfg(feature = "test-support")]
    FormHandoff {
        retained: bool,
    },
    /// A script form shown through data-driven UI.
    #[cfg(feature = "test-support")]
    DataDriven(protocol::ddui::ScriptedScreen),
    /// An edited settings draft after switching away from and back to its tab.
    #[cfg(feature = "test-support")]
    SettingsDraft,
    /// Retained Add Server focus after a simulated keyboard reduces the visible height.
    #[cfg(feature = "test-support")]
    MenuKeyboard,
    /// A damaged stack picked up from the survival inventory.
    #[cfg(feature = "test-support")]
    CursorDurability,
    /// Survival recipe-book inputs admitted by the inventory session and wire decoder.
    #[cfg(feature = "test-support")]
    CraftOverlap {
        selected: bool,
        block_identity: i32,
    },
    /// A client toast as the toast screen shows it once slid in.
    Toast {
        title: String,
        message: String,
    },
}

const SIZE: [u32; 2] = [1280, 720];
const GUI_SCALE: u8 = 2;
const SMALL_SIZE: [u32; 2] = [360, 240];
/// Fits an expanded card above two collapsed ones.
const TALL_SIZE: [u32; 2] = [1280, 900];
/// Keeps the first hotbar rows visible below the keyboard options.
const KEYBOARD_SIZE: [u32; 2] = [SIZE[0], 2000];
/// Reaches the Video pane's Renderer row below the general options.
const SHADER_PACK_ROW_SIZE: [u32; 2] = [SIZE[0], 1600];

/// Enumerates every fixture, including the independently owned settings states.
pub fn states() -> impl Iterator<Item = &'static State> {
    registry::STATES
        .iter()
        .chain(armor_trims::STATES)
        .chain(out_of_game::STATES)
        .chain(out_of_game::WORLD_FORM_STATES)
        .chain(graphics_recovery::STATES)
        .chain(updates::STATES)
        .chain(shader_pack::STATES)
        .chain(audio_output::STATES)
        .chain(social_settings::STATES)
        .chain(settings_screens::SEARCH_STATES)
        .chain(settings_screens::WORLD_STATES)
        .chain(settings_screens::PAGE_STATES)
        .chain(server_forms::STATES)
        .chain(hud_hotbar::STATES)
        .chain(touch_editor::STATES)
        .chain(mods::STATES)
}
