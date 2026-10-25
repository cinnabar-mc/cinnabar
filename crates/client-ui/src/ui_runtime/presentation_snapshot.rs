//! The display state that outbound producers may change within a frame.
use super::{UiRuntime, forms::ServerFormStore};

pub struct PresentationInventory {
    ledger: player_state::CapturedLedger,
    ui: CapturedUi,
}

/// UI-owned display fields written by outbound producers.
struct CapturedUi {
    forms: ServerFormStore,
    open: bool,
    pointer: Option<[f32; 2]>,
    controller_focus: Option<String>,
    generation: u64,
    controller_cursor: bool,
}

impl CapturedUi {
    fn swap(&mut self, runtime: &mut UiRuntime) {
        std::mem::swap(&mut self.forms, &mut runtime.forms);
        std::mem::swap(&mut self.open, &mut runtime.inventory_open);
        std::mem::swap(&mut self.pointer, &mut runtime.inventory_pointer_gui);
        std::mem::swap(
            &mut self.controller_focus,
            &mut runtime.inventory_controller_focus,
        );
        std::mem::swap(&mut self.generation, &mut runtime.container_generation);
        std::mem::swap(
            &mut self.controller_cursor,
            &mut runtime.inventory_controller_cursor_visible,
        );
    }
}

struct RestoreUi<'a> {
    runtime: &'a mut UiRuntime,
    after_send: CapturedUi,
}

impl Drop for RestoreUi<'_> {
    /// Restores post-send UI state even if rendering returns early or panics.
    fn drop(&mut self) {
        self.after_send.swap(self.runtime);
    }
}

impl UiRuntime {
    /// Captures inventory display state, sharing immutable item and creative catalogs.
    pub fn capture_presentation_inventory(
        &self,
        player_runtime: &player_state::PlayerState,
    ) -> PresentationInventory {
        PresentationInventory {
            ledger: player_runtime.capture_ledger(),
            ui: CapturedUi {
                forms: self.forms.clone(),
                open: self.inventory_open,
                pointer: self.inventory_pointer_gui,
                controller_focus: self.inventory_controller_focus.clone(),
                generation: self.container_generation,
                controller_cursor: self.inventory_controller_cursor_visible,
            },
        }
    }

    /// Renders the pre-send inventory from the snapshot; player authority is only read.
    pub fn with_presentation_inventory<T>(
        &mut self,
        player_runtime: &player_state::PlayerState,
        snapshot: PresentationInventory,
        render: impl FnOnce(&UiRuntime, &player_state::PlayerState) -> T,
    ) -> T {
        let presented = player_runtime.present(snapshot.ledger);
        let mut ui = snapshot.ui;
        ui.swap(self);
        let restore = RestoreUi {
            runtime: self,
            after_send: ui,
        };
        render(restore.runtime, &presented)
    }
}
