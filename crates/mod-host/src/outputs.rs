//! Committed visual outputs of a loaded component.

use super::{CameraDelta, ModHost};
use anyhow::Result;

impl ModHost {
    /// Reads a committed pulse so filtering can run before the frame outputs are collected.
    pub fn attack_pulse_requested(&self) -> bool {
        self.instance.attack_pulse_requested()
    }

    /// Successfully committed local weather target, in rain/thunder channel order.
    pub fn weather_override(&self) -> Option<[f32; 2]> {
        use crate::runtime::cinnabar::extension::environment::Weather;
        self.instance.weather_override().map(|value| match value {
            Weather::Clear => [0., 0.],
            Weather::Rain => [1., 0.],
            Weather::Thunder => [1., 1.],
        })
    }
    /// Committed touch buttons; empty for an inactive instance or without controls authority.
    pub fn touch_buttons(&self) -> &[ui::mod_touch::TouchButton] {
        self.instance.touch_buttons()
    }
    /// Identifies this component instance to presented buttons; a reload gets a new token.
    pub fn touch_owner(&self) -> u64 {
        self.instance.touch_owner()
    }
    /// Whether the last executed frame callback received taps, including deferred ones.
    pub fn touch_presses_delivered(&self) -> bool {
        self.instance.touch_presses_delivered()
    }
    /// Queues a completed tap on one of this mod's buttons for its next frame callback.
    /// Returns false when the button is gone or the bounded queue is full.
    pub fn queue_touch_press(&mut self, id: &str) -> bool {
        self.instance.queue_touch_press(id)
    }
    /// Retained attack/use caps; an absent channel leaves ordinary input unchanged.
    pub fn click_limits(&self) -> [Option<u32>; 2] {
        self.instance.click_limits()
    }

    /// Current-frame ordinary input modes; absence restores the player's saved preferences.
    pub fn movement_toggles(&self) -> Option<&crate::MovementToggles> {
        self.instance.movement_toggles()
    }
    /// Successfully committed local lighting override.
    pub fn fullbright(&self) -> bool {
        self.instance.fullbright()
    }

    /// Committed selection; no raw block reads are exposed to the component.
    pub fn block_highlights(&self) -> Option<&mod_api::BlockHighlightSpec> {
        self.instance.block_highlights()
    }

    /// Explicit opt-in to the private core's last-relayed local position witness.
    pub fn show_real_position(&self) -> bool {
        self.instance.show_real_position()
    }

    /// Consumes the last successful frame's rotation once, without entering the guest.
    pub fn take_camera_delta(&mut self) -> Option<CameraDelta> {
        self.instance.take_camera_delta()
    }

    /// Committed render output and a process-unique generation that changes with it.
    pub fn render(&self) -> (&mod_render::RenderOutput, u64) {
        self.instance.render()
    }

    /// Returns only the last successfully committed plain-text label.
    pub fn label(&self) -> Option<&str> {
        self.instance.label()
    }

    /// Retained host-rendered cards from the last successful callback.
    pub fn hud(&self) -> Option<&ui::mod_hud::Hud> {
        self.instance.hud()
    }

    /// Process-unique committed HUD identity, including changes from native bindings.
    pub fn hud_revision(&self) -> Option<u64> {
        self.hud().map(|_| self.instance.hud_revision())
    }

    /// Consumes a committed native-editor request without entering the guest.
    pub fn take_hud_editor_request(&mut self) -> Option<ui::mod_hud::Hud> {
        self.instance.take_hud_editor_request()
    }

    /// Delivers host-owned layout output to this instance only.
    pub fn deliver_hud_editor_result(&mut self, result: ui::mod_hud::EditorResult) -> Result<()> {
        result.validate().map_err(anyhow::Error::msg)?;
        if self.is_active() {
            self.instance.deliver_hud_editor_result(result);
        }
        Ok(())
    }

    /// Retained cosmetic crosshair, applied only when the ordinary crosshair is visible.
    pub fn crosshair(&self) -> Option<&ui::mod_hud::Crosshair> {
        self.instance.crosshair()
    }

    /// Returns the committed visual override without entering the guest.
    pub fn time_override(&self) -> Option<u32> {
        self.instance.time_override()
    }
}
