//! Presentation of the surfaces mods and server experiences own: trusted consent chrome, a
//! client part's modal, player-mod screens over containers, the personal panel, the host
//! layout editor, and mods' HUD surfaces and label. Screens paint through `ui-engine`
//! from borrowed inputs; the host projects runtime authority into them.

mod experience;
pub mod experience_modal;
pub mod fonts;
mod hud_surface;
pub use hud_surface::{ActorAuthority, ProjectedActor, PublishedHudSurface};
mod label_metrics;
pub use label_metrics::LabelMetrics;
mod local_player_identity;
mod mod_hud;
mod mod_hud_editor;
pub use mod_hud_editor::HudEditor;
mod mod_panel;
pub use mod_panel::prepared::{PanelUiOwner, PreparedPanelUi};
mod mod_presentation;
pub mod mod_screens;
mod mod_widgets;
pub mod template_screen;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

/// Counts allocations for the crate's zero-allocation steady-frame tests.
#[cfg(test)]
#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

/// Every mod- and experience-owned surface's state, carried across builds.
#[derive(Default)]
pub struct ModUi {
    mod_hud: Option<mod_hud::ModHud>,
    /// Vanilla HUD elements mods hide.
    hidden_elements: ui::mod_hud::HiddenElements,
    /// Mods' HUD surfaces.
    hud_surfaces: hud_surface::HudSurfaces,
    /// Which actors the selected panel's projections may follow.
    panel_actor_authority: ActorAuthority,
    mod_panel: Option<mod_panel::ModPanel>,
    local_player_identity: local_player_identity::LocalPlayerIdentity,
    prepared_panel_ui: std::collections::BTreeMap<PanelUiOwner, mod_panel::prepared::PreparedOwner>,
    experience: Option<experience::ExperienceChrome>,
    /// A client part's modal screen.
    experience_modal: Option<experience_modal::ModalScreen>,
    /// A player mod's overlay and view.
    mod_screens: Option<mod_screens::ModScreens>,
    /// An explicit embedding override settles mod motion independently of built-in screens.
    reduced_motion: bool,
    /// Every mod's native presentation parameters, merged in load order.
    mod_presentation: std::sync::Arc<ui::mod_presentation::Presentation>,
}

/// Shares the immutable default context used by retained client-owned mod surfaces.
fn personal_context() -> &'static json_ui::Context {
    static CONTEXT: std::sync::OnceLock<json_ui::Context> = std::sync::OnceLock::new();
    CONTEXT.get_or_init(json_ui::Context::default)
}
