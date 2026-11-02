use bevy::prelude::Resource;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
/// True once every registered variant for the current views has compiled.
#[derive(Resource, Clone, Debug, Default)]
pub struct PipelineWarmupReadiness(Arc<AtomicBool>);

impl PipelineWarmupReadiness {
    /// Publishes whether all registered pipeline variants are ready for the current view.
    pub fn set_ready(&self, ready: bool) {
        self.0.store(ready, Ordering::Release);
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// A sample count the world camera has been asked to use but does not draw yet.
///
/// The main world requests it; the render world compiles every warm-up variant for it
/// in the background and then reports it ready, so the camera keeps drawing at its old
/// count instead of skipping draws while new pipelines compile.
#[derive(Resource, Clone, Debug, Default)]
pub struct SampleCountPreflight {
    requested: Option<bevy::render::view::Msaa>,
    ready: Arc<AtomicU32>,
}

impl SampleCountPreflight {
    /// Replaces the pending request; `None` withdraws it.
    pub fn request(&mut self, msaa: Option<bevy::render::view::Msaa>) {
        self.requested = msaa;
    }

    /// The sample count waiting for its pipelines, if any.
    #[must_use]
    pub fn requested(&self) -> Option<bevy::render::view::Msaa> {
        self.requested
    }

    /// Render world: publishes the count whose variants have all compiled, or none.
    pub fn publish_ready(&self, msaa: Option<bevy::render::view::Msaa>) {
        self.ready
            .store(msaa.map_or(0, |msaa| msaa.samples()), Ordering::Release);
    }

    /// Whether the render world last reported `msaa`'s variants as compiled.
    #[must_use]
    pub fn is_ready(&self, msaa: bevy::render::view::Msaa) -> bool {
        self.ready.load(Ordering::Acquire) == msaa.samples()
    }
}
