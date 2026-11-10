//! Connects mod consent and managed-world observations without owning terrain policy.
use super::LocalWorlds;
use std::sync::Arc;

impl LocalWorlds {
    /// The exact playing local world's source; the app forwards it only to authorized readers.
    pub fn local_terrain_source(
        &self,
        identity: &str,
    ) -> Option<Arc<dyn mod_api::local_terrain::Source>> {
        if !self.playing {
            return None;
        }
        self.manager.as_ref()?.local_terrain_source(identity)
    }

    /// Carries consent to a manager attached later, before the player opens a world.
    pub fn set_local_terrain_enabled(&mut self, enabled: bool) {
        self.local_terrain_enabled = enabled;
        if let Some(manager) = &self.manager {
            manager.set_local_terrain_enabled(enabled);
        }
    }
}
