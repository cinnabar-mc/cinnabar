use bevy::prelude::Resource;
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, Ordering},
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

/// The pipeline-relevant camera configuration for one requested sample count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleCountRequest {
    pub samples: bevy::render::view::Msaa,
    pub hdr: bool,
    pub enhanced: bool,
}

impl SampleCountRequest {
    /// Returns the requested count only for a view with the matching rendering configuration.
    pub fn for_view(self, hdr: bool, enhanced: bool) -> Option<bevy::render::view::Msaa> {
        (self.hdr == hdr && self.enhanced == enhanced).then_some(self.samples)
    }
}

/// A sample count the world camera has been asked to use but does not draw yet.
///
/// The main world requests it; the render world compiles every warm-up variant for it
/// in the background and then reports it ready, so the camera keeps drawing at its old
/// count instead of skipping draws while new pipelines compile.
#[derive(Resource, Clone, Debug, Default)]
pub struct SampleCountPreflight {
    requested: Option<SampleCountRequest>,
    ready: Arc<Mutex<Option<SampleCountRequest>>>,
}

impl SampleCountPreflight {
    /// Replaces the pending request; `None` withdraws it.
    pub fn request(&mut self, request: Option<SampleCountRequest>) {
        self.requested = request;
    }

    /// The sample count waiting for its pipelines, if any.
    #[must_use]
    pub fn requested(&self) -> Option<SampleCountRequest> {
        self.requested
    }

    /// Render world: publishes the configuration whose variants have all compiled, or none.
    pub fn publish_ready(&self, request: Option<SampleCountRequest>) {
        *self.ready.lock().unwrap_or_else(PoisonError::into_inner) = request;
    }

    /// Whether the render world reported this exact configuration's variants as compiled.
    #[must_use]
    pub fn is_ready(&self, request: SampleCountRequest) -> bool {
        *self.ready.lock().unwrap_or_else(PoisonError::into_inner) == Some(request)
    }
}
