//! Card artwork the host layout editor previews, and the vanilla HUD elements mods hide.

pub(crate) mod cells;
pub(super) mod template;

use crate::ModUi;
use ui::mod_hud::HiddenElements;

impl ModUi {
    /// Vanilla HUD elements loaded mods hide, combined.
    pub fn set_hidden_hud_elements(&mut self, hidden: HiddenElements) {
        self.hidden_elements = hidden;
    }

    /// Vanilla HUD elements not drawn while the gameplay HUD shows.
    pub fn hidden_hud_elements(&self) -> HiddenElements {
        self.hidden_elements
    }
}
