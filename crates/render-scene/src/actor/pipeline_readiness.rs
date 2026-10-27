use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use bevy::prelude::Resource;

/// Readiness of all actor raster contracts for the active render views.
#[derive(Clone, Debug, Default, Resource)]
pub struct ActorPipelineReadiness {
    ready: Arc<AtomicBool>,
    /// Distinct actor materials drawn with their view's unauthored pipeline because the render
    /// targets cannot represent them or their pipeline failed to compile.
    material_fallbacks: Arc<AtomicU32>,
}

impl ActorPipelineReadiness {
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    pub fn publish(&self, ready: bool) {
        self.ready.store(ready, Ordering::Release);
    }

    /// Distinct actor materials currently drawn with the unauthored pipeline instead.
    #[must_use]
    pub fn material_fallbacks(&self) -> u32 {
        self.material_fallbacks.load(Ordering::Acquire)
    }

    /// Publishes the render world's count of actor material fallbacks.
    pub fn publish_material_fallbacks(&self, count: u32) {
        self.material_fallbacks.store(count, Ordering::Release);
    }
}
