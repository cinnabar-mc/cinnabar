//! Card artwork the host layout editor previews, and the vanilla HUD elements mods hide.

pub(crate) mod cells;
pub(super) mod template;

use crate::ModUi;
use ui::mod_hud::{ElementPlacement, HiddenElements};

impl ModUi {
    /// Vanilla HUD elements loaded mods hide, combined.
    pub fn set_hidden_hud_elements(&mut self, hidden: HiddenElements) {
        self.hidden_elements = hidden;
    }

    /// Vanilla HUD elements not drawn while the gameplay HUD shows.
    pub fn hidden_hud_elements(&self) -> HiddenElements {
        self.hidden_elements
    }

    /// Where loaded mods draw the vanilla sidebar.
    pub fn set_sidebar_placement(&mut self, placement: Option<ElementPlacement>) {
        self.sidebar_placement = placement;
    }

    /// Where the vanilla sidebar draws instead of where it is laid out.
    pub fn sidebar_placement(&self) -> Option<ElementPlacement> {
        self.sidebar_placement
    }
}
