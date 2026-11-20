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

/// Shows the notice for a launch at `level` after the renderer ended the previous run.
fn safe_graphics(level: render_api::SafeGraphics, kept: bool) -> Fixture {
    let mut view = MenuView::new(true, "Player".into());
    view.dialog = Some(MenuDialog::Notice);
    view.message = launcher::graphics_backend::safe_graphics_notice(level, kept);
    Fixture::Menu(Box::new(view))
}

/// The first step: nothing visible is reduced, only loading is slower.
fn safe_graphics_first_level() -> Fixture {
    safe_graphics(render_api::SafeGraphics::FreshPipelines, false)
}

/// The last step, where graphics are visibly reduced.
fn safe_graphics_reduced() -> Fixture {
    safe_graphics(render_api::SafeGraphics::MAX, false)
}

/// The last step kept after full graphics failed again.
fn safe_graphics_kept() -> Fixture {
    safe_graphics(render_api::SafeGraphics::MAX, true)
}

pub(super) const STATES: &[State] = &[
    State {
        name: "graphics-api-recovery",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: notice,
    },
    State {
        name: "safe-graphics-first-level",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: safe_graphics_first_level,
    },
    State {
        name: "safe-graphics-reduced",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: safe_graphics_reduced,
    },
    State {
        name: "safe-graphics-kept",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: safe_graphics_kept,
    },
];
