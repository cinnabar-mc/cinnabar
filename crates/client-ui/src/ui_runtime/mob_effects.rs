//! Vanilla's mob effect screen: whether it is open, and the local effects it lists.

use ui_screens::mob_effect_screen::MobEffectEntry;

use super::UiRuntime;

/// Opened over gameplay by the Mob Effects binding; closed by its own buttons, Escape or damage.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MobEffectScreenState {
    open: bool,
}

impl MobEffectScreenState {
    pub const fn is_open(&self) -> bool {
        self.open
    }

    pub fn close(&mut self) {
        self.open = false;
    }
}

impl UiRuntime {
    pub const fn mob_effect_screen(&self) -> &MobEffectScreenState {
        &self.mob_effect_screen
    }

    pub fn close_mob_effect_screen(&mut self) {
        self.mob_effect_screen.close();
    }

    /// The binding's press: closes an open screen, else opens it as
    /// `ClientInputCallbacks::handleMobEffectsButtonPress` does, only while the local player
    /// has an active effect. Returns whether the screen is open afterwards.
    pub fn toggle_mob_effect_screen(&mut self, now_millis: u64) -> bool {
        self.mob_effect_screen.open =
            !self.mob_effect_screen.open && !self.mob_effect_entries(now_millis).is_empty();
        self.mob_effect_screen.open
    }

    /// The local player's active effects at `now_millis`, iconless ones included.
    pub fn mob_effect_entries(&self, now_millis: u64) -> Vec<MobEffectEntry> {
        let now_tick = self.estimated_server_tick(now_millis);
        self.gameplay_hud()
            .effects()
            .iter()
            .filter(|effect| effect.visible_at_tick(now_tick))
            .map(|effect| MobEffectEntry {
                effect_id: effect.effect_id,
                amplifier: effect.amplifier,
                ambient: effect.ambient,
                // Applying an effect starts the presentation clock, so a timed effect
                // without one is not observable; it would list as expired.
                remaining_ticks: effect
                    .expires_at_tick
                    .map(|_| effect.remaining_ticks(now_tick).unwrap_or(0)),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
