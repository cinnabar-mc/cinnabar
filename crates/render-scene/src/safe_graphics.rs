//! This launch's safe graphics level, for the plugins that reduce what they draw.

use bevy::prelude::{App, Resource};
use render_api::SafeGraphics;

/// The level chosen before the renderer starts; it never changes during a launch.
#[derive(Resource, Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ActiveSafeGraphics(pub SafeGraphics);

impl ActiveSafeGraphics {
    /// The level `app` was started with; an app that was given none draws full graphics.
    pub fn of(app: &App) -> SafeGraphics {
        app.world()
            .get_resource::<Self>()
            .map_or(SafeGraphics::Off, |active| active.0)
    }
}
