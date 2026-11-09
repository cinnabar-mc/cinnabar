//! Projects runtime authority into `mod-ui`'s surfaces and paints them in the scene order the
//! frame builder chooses.

use launcher::menu::settings_options::SettingId;
use std::collections::BTreeSet;

use mod_ui::{ModUi, experience_modal::ExperienceModal, mod_screens::ModScreensInput};
use ui::UiNode;
use ui_engine::{engine::screen::PaintFrame, pages, text_metrics::TextMetrics};

use super::super::UiPresentationRuntime;
use crate::ui_runtime::UiRuntime;

/// A paint frame over the runtime's text caches, painting at time zero with no renderers.
macro_rules! paint_frame {
    ($runtime:ident, $ui:expr, $nodes:expr, $next:expr, $metrics:expr, $content:expr, $translate:expr) => {
        PaintFrame {
            layouts: &mut $runtime.layouts,
            font: &$runtime.font,
            metrics: $metrics,
            solid_page: $runtime.solid_texture_page,
            safe_area: $runtime.safe_area,
            content: $content,
            translate: $translate,
            language: $ui.text_generation(),
            now: $runtime.menu_seconds,
            nodes: $nodes,
            next: $next,
            renderers: &mut [],
        }
    };
}

#[cfg(test)]
mod tests;

impl UiPresentationRuntime {
    /// Every mod- and experience-owned surface.
    pub fn mod_ui(&self) -> &ModUi {
        &self.screens.mod_ui
    }

    pub fn mod_ui_mut(&mut self) -> &mut ModUi {
        &mut self.screens.mod_ui
    }

    /// Replaces the merged native presentation parameters, first revoking chat links bound to
    /// chat actions the new parameters no longer publish.
    pub fn set_mod_presentation(
        &mut self,
        presentation: std::sync::Arc<ui::mod_presentation::Presentation>,
    ) {
        if self.screens.mod_ui.set_mod_presentation(presentation) {
            self.screens.chat.forget_annotations();
        }
    }

    /// Follows the client part's modal in the dynamic pages reserved to its atlas.
    pub fn set_experience_modal(&mut self, modal: Option<ExperienceModal<'_>>) {
        let page = (self.textures.dynamic_start() + pages::MODAL_UI_PAGE) as u16;
        self.screens.mod_ui.set_experience_modal(modal, page);
    }

    /// The player inventory slot of the client part modal's read-only inventory or hotbar rows
    /// under window-logical `point`, which a hotbar key acts on as in a container screen; none
    /// while one of its edit boxes takes typing.
    pub fn experience_modal_inventory_hit(
        &self,
        point: [f32; 2],
    ) -> Option<ui_screens::container_actions::InventoryCellHit> {
        let modal = &self.screens.mod_ui;
        if modal.experience_modal_text_focused() {
            return None;
        }
        let (collection, index) = modal.experience_modal_cell_at(point)?;
        ui_screens::containers::player_cell(collection, index)
    }

    /// Follows the player mod's screens in the dynamic pages reserved to their atlas.
    pub fn set_mod_screens(&mut self, input: Option<ModScreensInput<'_>>) {
        let page = (self.textures.dynamic_start() + pages::MOD_UI_PAGE) as u16;
        self.screens.mod_ui.set_mod_screens(input, page);
    }

    /// All personal HUD content follows gameplay focus and the player's HUD preference, and
    /// stays under an open panel that keeps the gameplay HUD, as vanilla's HUD does.
    pub(in super::super) fn mod_hud_visible(
        &self,
        player: &player_state::PlayerState,
        runtime: &UiRuntime,
    ) -> bool {
        let mod_ui = &self.screens.mod_ui;
        !runtime.ui_focused(player)
            && self.menu_view.is_none()
            && self.loading_stage.is_none()
            && self
                .screens
                .chat
                .settings
                .options()
                .value(SettingId::HideHud)
                == 0
            && (!mod_ui.mod_panel_open() || mod_ui.mod_panel_shows_gameplay_hud())
    }

    /// Draws the extension label through JSON-UI only while the gameplay HUD owns focus.
    pub(in super::super) fn append_mod_hud(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) {
        if !self.mod_hud_visible(player_runtime, runtime)
            || runtime.ui_focused(player_runtime)
            || self.loading_stage.is_some()
            || !self.screens.hud.hud.has_visible_content()
        {
            return;
        }
        let Some(renderer) = self.engine.as_deref() else {
            return;
        };
        let mut paint = paint_frame!(self, runtime, nodes, next, metrics, content, &|_| None);
        self.screens.mod_ui.paint_mod_hud(renderer, &mut paint);
    }

    /// Draws mods' HUD surfaces while the HUD shows content.
    pub(in super::super) fn append_hud_surfaces(
        &mut self,
        player: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) {
        self.refresh_label_metrics(metrics);
        if !self.mod_hud_visible(player, runtime) || !self.screens.hud.hud.has_visible_content() {
            return;
        }
        let translate = |key: &str| runtime.translation(key);
        let session_icons = &self.session_icons;
        let icon_catalog = self.icon_catalog.as_deref();
        let icon_refs = self.icon_refs.as_deref();
        let icon = |id: &str, metadata| {
            super::super::session_icons::lookup_icon(
                session_icons,
                icon_catalog,
                icon_refs,
                id,
                metadata,
            )
        };
        let mut paint = paint_frame!(self, runtime, nodes, next, metrics, content, &translate);
        self.screens
            .mod_ui
            .paint_hud_surfaces(self.engine.as_deref(), &mut paint, &icon);
    }

    /// Keeps the label metrics `hud.measure-text` reads on the HUD's font and display scale.
    fn refresh_label_metrics(&mut self, metrics: TextMetrics) {
        if !self
            .label_metrics
            .as_ref()
            .is_some_and(|label| label.matches(&self.font, metrics))
        {
            self.label_metrics = Some(std::sync::Arc::new(mod_ui::LabelMetrics::new(
                std::sync::Arc::clone(&self.font),
                metrics,
            )));
        }
    }

    /// Text metrics of the last painted HUD, for `hud.measure-text`.
    pub fn label_metrics(&self) -> Option<std::sync::Arc<mod_ui::LabelMetrics>> {
        self.label_metrics.clone()
    }

    /// The last painted HUD root and its vanilla elements, for `hud.read-viewport` and
    /// `hud.read-elements`.
    pub fn hud_elements(&self) -> (Option<ui::mod_hud::HudViewport>, ui::mod_hud::ElementStates) {
        (self.screens.mod_ui.hud_viewport(), self.hud_elements.states)
    }

    /// Draws the personal panel.
    pub(in super::super) fn append_mod_panel(
        &mut self,
        _player: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) {
        let translate = |key: &str| runtime.translation(key);
        let session_icons = &self.session_icons;
        let icon_catalog = self.icon_catalog.as_deref();
        let icon_refs = self.icon_refs.as_deref();
        let icon = |id: &str, metadata| {
            super::super::session_icons::lookup_icon(
                session_icons,
                icon_catalog,
                icon_refs,
                id,
                metadata,
            )
        };
        let mut paint = paint_frame!(self, runtime, nodes, next, metrics, content, &translate);
        self.screens
            .mod_ui
            .paint_mod_panel_with_icons(self.engine.as_deref(), &mut paint, &icon);
    }

    /// Draws the client part's modal over the gameplay scenes; trusted chrome draws after it.
    #[allow(
        clippy::too_many_arguments,
        reason = "Player authority is borrowed separately from UI state."
    )]
    pub(in super::super) fn append_experience_modal(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        over_gameplay: bool,
    ) {
        let id_aux = match self.screens.mod_ui.experience_modal_icon_keys() {
            Some(keys) => item_icons(keys, player_runtime, runtime, |id, aux| {
                self.item_icon(id, aux)
            }),
            None => Vec::new(),
        };
        let translate = |key: &str| runtime.translation(key);
        let hud_frame = &self.hud_frame;
        let mut paint = paint_frame!(self, runtime, nodes, next, metrics, content, &translate);
        self.screens.mod_ui.paint_experience_modal(
            self.engine.as_deref(),
            &mut paint,
            over_gameplay,
            &id_aux,
            |icons| super::containers::player_rows(player_runtime, hud_frame, icons),
        );
    }

    /// Draws trusted chrome last, including when server UI or the HUD is hidden.
    pub(in super::super) fn append_experience_chrome(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) {
        let mut paint = paint_frame!(self, runtime, nodes, next, metrics, content, &|_| None);
        self.screens
            .mod_ui
            .paint_experience_chrome(self.engine.as_deref(), &mut paint);
    }

    /// Draws the mod's view and overlay over the container screen this build drew, from node
    /// `container_start` on.
    #[allow(
        clippy::too_many_arguments,
        reason = "Player authority is borrowed separately from UI state."
    )]
    pub(in super::super) fn append_mod_screens(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        container_start: usize,
    ) {
        let Some(keys) = self.screens.mod_ui.mod_screen_icon_keys() else {
            return;
        };
        let id_aux = item_icons(keys, player_runtime, runtime, |id, aux| {
            self.item_icon(id, aux)
        });
        let translate = |key: &str| runtime.translation(key);
        let container = self.projections.container.as_ref().map(|(frame, _)| frame);
        let mut paint = paint_frame!(self, runtime, nodes, next, metrics, content, &translate);
        self.screens.mod_ui.paint_mod_screens(
            self.engine.as_deref(),
            container,
            &mut paint,
            container_start,
            &id_aux,
            || super::container_screen_reference(player_runtime, runtime),
        );
    }
}

/// Each key's icon: vanilla's `#item_id_aux` is the network id over 16 bits of aux, which the
/// session's item registry names.
fn item_icons(
    keys: &BTreeSet<i64>,
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    icon: impl Fn(&str, u32) -> Option<ui::IconRef>,
) -> Vec<(i64, ui::IconRef)> {
    let Some(registry) = runtime
        .inventory_ledger(player_runtime)
        .negotiated_item_registry()
    else {
        return Vec::new();
    };
    keys.iter()
        .filter_map(|key| {
            let network_id = i32::try_from(key >> 16).ok()?;
            let aux = u32::try_from(key & 0xffff).ok()?;
            let entry = registry.get(&network_id)?;
            Some((*key, icon(&entry.identifier, aux)?))
        })
        .collect()
}
