//! Resolved screen flags shared by input and rendering.
use super::{Context, FormEngine};

impl FormEngine {
    /// Reads the active catalog's root flags without resolving unused descendant controls.
    pub fn scene_settings(&self, reference: &str, context: &Context) -> json_ui::ScreenSettings {
        let catalog = self.screen_catalog(reference);
        self.screens.settings(reference, catalog, context)
    }
}

#[cfg(test)]
mod tests;
