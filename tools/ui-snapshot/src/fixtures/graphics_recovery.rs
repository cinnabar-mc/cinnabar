//! Anonymous renderer fallback notices for completed and interrupted launches.

use super::{Fixture, GUI_SCALE, MenuView, SIZE, State};
use launcher::{
    graphics_backend::{Backend, PREVIOUS_FAILURE_REASON, Platform, fallback_notice},
    menu::MenuDialog,
};

/// Shows the production recovery notice after the saved DX12 choice falls back to Vulkan.
fn notice() -> Fixture {
    let mut view = MenuView::new(true, "Player".into());
    view.dialog = Some(MenuDialog::Notice);
    view.message = Some(fallback_notice(
        Backend::Dx12,
        Backend::Vulkan,
        Platform::Windows,
        PREVIOUS_FAILURE_REASON,
    ));
    Fixture::Menu(Box::new(view))
}

pub(super) const STATES: &[State] = &[State {
    name: "graphics-api-recovery",
    size: SIZE,
    gui_scale: GUI_SCALE,
    fixture: notice,
}];
