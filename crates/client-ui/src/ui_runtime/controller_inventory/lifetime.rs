//! Container open-instance identity and presentation ownership.

use crate::ui_runtime::UiRuntime;

impl UiRuntime {
    /// Identifies one open instance, including close/reopen within one input frame.
    pub const fn container_generation(&self) -> u64 {
        self.container_generation
    }

    /// Invalidates retained geometry and input without changing ledger authority.
    pub(in crate::ui_runtime) fn advance_container_generation(&mut self) {
        self.container_generation = self
            .container_generation
            .checked_add(1)
            .expect("container generation exhausted");
        self.inventory_pointer_gui = None;
        self.inventory_controller_focus = None;
        self.inventory_controller_cursor_visible = false;
    }

    /// Records every open/close edge at its mutation site, not at frame publication.
    pub(crate) fn set_inventory_open(&mut self, open: bool) {
        if self.inventory_open == open {
            return;
        }
        self.advance_container_generation();
        self.inventory_open = open;
    }

    /// The controller highlight key; text-entry focus remains separately owned.
    pub fn inventory_controller_focus(&self) -> Option<&str> {
        self.inventory_controller_focus.as_deref()
    }

    /// Publishes directional controller focus without taking text-entry ownership.
    pub fn set_inventory_controller_focus(&mut self, key: Option<String>) {
        self.inventory_controller_focus = key;
    }

    /// Whether presentation should draw the client-owned controller GUI pointer.
    pub const fn inventory_controller_cursor_visible(&self) -> bool {
        self.inventory_controller_cursor_visible
    }

    /// Changes cursor presentation only; never injects an OS pointer event.
    pub fn set_inventory_controller_cursor_visible(&mut self, visible: bool) {
        self.inventory_controller_cursor_visible = visible;
    }
}
