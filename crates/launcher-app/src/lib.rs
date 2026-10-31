//! The standalone desktop launcher: a window on the shared bootstrap stack with the signed-in
//! account, saved servers and installation details, which starts or joins with the game client.

mod app;
pub mod play;
pub mod screen;
pub mod snapshot;
mod theme;
pub mod view;

use bootstrap_ui::input::Actions;
use launcher::install_layout::InstallLayout;
use winit::{
    dpi::LogicalSize,
    event_loop::{ControlFlow, EventLoop},
};

use screen::{Action, Screen};

/// No window could open, or the installation could not be read.
pub const EXIT_UNAVAILABLE: i32 = 4;
/// The launcher window's default logical size.
const WINDOW_SIZE: LogicalSize<f64> = LogicalSize::new(1200.0, 720.0);

impl Actions for Screen {
    type Action = Action;

    fn actions(&self) -> &[Action] {
        Screen::actions(self)
    }

    /// Escape and the controller's back button close the failure modal; elsewhere the screen
    /// does not offer Dismiss, so they do nothing.
    fn cancel(&self) -> Action {
        Action::Dismiss
    }
}

/// Opens the launcher window for this executable's installation; returns the exit code.
#[expect(
    clippy::print_stderr,
    reason = "the launcher installs no log subscriber; stderr is its only diagnostic channel"
)]
pub fn run() -> i32 {
    let layout = match InstallLayout::discover() {
        Ok(layout) => layout,
        Err(error) => {
            eprintln!("launcher unavailable: {error}");
            return EXIT_UNAVAILABLE;
        }
    };
    let text = match bootstrap_ui::text(&layout) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("launcher unavailable: no usable UI font ({error:#})");
            return EXIT_UNAVAILABLE;
        }
    };
    let Some(faces) = bootstrap_ui::built_in_faces() else {
        return EXIT_UNAVAILABLE;
    };
    let Ok(event_loop) = EventLoop::new() else {
        return EXIT_UNAVAILABLE;
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = app::LauncherApp::new(layout, text, faces);
    let failed = event_loop.run_app(&mut app).is_err();
    app.exit_code()
        .unwrap_or(if failed { EXIT_UNAVAILABLE } else { 0 })
}
