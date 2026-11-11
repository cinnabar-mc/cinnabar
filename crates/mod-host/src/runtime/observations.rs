//! Validation and publication of host-owned callback observations.

use super::*;

impl Instance {
    /// Validates presentation observations before they enter the callback sandbox.
    pub(crate) fn observe_presentation(
        &mut self,
        world: Option<crate::WorldObservation>,
        fps: Option<f32>,
        sidebar: Option<crate::SidebarSnapshot>,
        chat: Vec<crate::ObservedChatMessage>,
    ) -> Result<()> {
        anyhow::ensure!(
            world.as_ref().is_none_or(|w| w.session != 0
                && !w.world.is_empty()
                && w.world.len() <= mod_api::MAX_WORLD_IDENTITY_BYTES
                && !w.world.chars().any(char::is_control)
                && [w.position.0, w.position.1, w.position.2]
                    .iter()
                    .all(|v| v.is_finite())),
            "invalid world observation"
        );
        anyhow::ensure!(
            fps.is_none_or(|value| value.is_finite() && (0.0..=100000.0).contains(&value)),
            "invalid frame observation"
        );
        anyhow::ensure!(
            sidebar.as_ref().is_none_or(|s| s.rows.len() <= 15
                && s.title.len() <= ui::mod_hud::MAX_OBSERVED_TEXT_BYTES
                && s.rows
                    .iter()
                    .all(|r| r.label.len() <= ui::mod_hud::MAX_OBSERVED_TEXT_BYTES)),
            "invalid sidebar observation"
        );
        anyhow::ensure!(
            chat.len() <= ui::mod_hud::MAX_OBSERVED_CHAT_MESSAGES
                && chat
                    .iter()
                    .all(|m| m.text.len() <= ui::mod_hud::MAX_OBSERVED_TEXT_BYTES)
                && chat
                    .windows(2)
                    .all(|pair| pair[0].sequence < pair[1].sequence),
            "invalid chat observation"
        );
        if self
            .continuation
            .as_ref()
            .is_some_and(|call| call.observed_world != world.as_ref().map(|world| world.session))
        {
            self.cancel_continuation();
        }
        let state = self.store.data_mut();
        if state.world_output_available() {
            self.world_entry
                .observe(world.as_ref().map(|world| world.session));
        }
        state.player_state.world = world;
        state.player_state.fps = fps;
        state.player_state.sidebar = sidebar;
        state.player_state.chat = chat;
        Ok(())
    }

    /// Validates native cosmetic facts before exposing any current callback data.
    pub(crate) fn observe_modules(
        &mut self,
        observations: crate::module_observations::ModuleObservations,
    ) -> Result<()> {
        crate::module_observations::validate(&observations)?;
        self.store.data_mut().player_state.observations = observations;
        Ok(())
    }

    /// Records the last laid-out HUD root and vanilla element states for later reads.
    pub(crate) fn observe_hud(
        &mut self,
        viewport: Option<ui::mod_hud::HudViewport>,
        elements: ui::mod_hud::ElementStates,
    ) {
        let hud = &mut self.store.data_mut().hud;
        hud.viewport = viewport.filter(|viewport| viewport.valid());
        hud.elements = elements.map(ui::mod_hud::ElementState::sanitized);
    }

    /// Records this mod's open panel's last press on a declared control, for `panel.read-press`.
    pub(crate) fn observe_panel_press(&mut self, press: Option<ui::mod_hud::PanelPress>) {
        self.store.data_mut().controls.press = press.filter(|press| {
            press
                .rect
                .iter()
                .chain(&press.to_local)
                .all(|value| value.is_finite())
                && press.rect[2] >= 0.
                && press.rect[3] >= 0.
        });
    }

    /// Records where the host last laid out this mod's open panel, for `panel.read-layout`.
    pub(crate) fn observe_panel_layout(&mut self, layout: Option<ui::mod_hud::PanelLayout>) {
        self.store.data_mut().controls.layout = layout.filter(|layout| layout.valid());
    }
}
