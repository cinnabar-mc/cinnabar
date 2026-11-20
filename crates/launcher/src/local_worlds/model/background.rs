//! What the worlds menu does when the host closed the open world while the app was off screen.

use super::{Effect, WorldsMenu};

/// Shown when the player returns to a world that was closed while the app was in the background.
pub const CLOSED_IN_BACKGROUND: &str = concat!(
    "This world was closed because the app stayed in the background. ",
    "Open it again to keep playing."
);

impl WorldsMenu {
    /// Ends an open that was still in progress, or that finished without its session starting
    /// yet, when the host closed its world while the app was off screen: the player returns to
    /// [`CLOSED_IN_BACKGROUND`]. Does nothing when no open is pending; a world already being
    /// played ends with its session.
    pub fn closed_in_background(&mut self) -> Vec<Effect> {
        let opening = self.opening.take().is_some();
        let ready = self.ready.take().is_some();
        if !(opening || ready) {
            return Vec::new();
        }
        self.opening_status = None;
        self.fail(CLOSED_IN_BACKGROUND);
        vec![Effect::Close]
    }
}
