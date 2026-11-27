//! The actor store drives bounded immutable preparation before choosing complete profiles.
use super::*;

impl ActorAnimationStore {
    /// Diagnostic stores have no catalog against which a custom model can be resolved.
    pub fn has_skin_preparation(&self) -> bool {
        self.assets.is_some()
    }

    /// Receives bounded worker results without waiting for an unfinished job.
    pub fn begin_skin_preparation(&mut self) {
        self.skin_preparation.begin_frame();
    }

    /// Admits only a retained source pointer; all content work belongs to the worker.
    pub fn request_skin_preparation(
        &mut self,
        source: &Arc<render_api::SkinGeometrySource>,
    ) -> bool {
        self.request_replacing_skin_preparation(source, None)
    }

    /// The worker compares replacement bytes against the still-ready result, never against a main-thread hash.
    pub fn request_replacing_skin_preparation(
        &mut self,
        source: &Arc<render_api::SkinGeometrySource>,
        previous: Option<&Arc<render_api::SkinGeometrySource>>,
    ) -> bool {
        self.skin_preparation.request_replacing(source, previous)
    }

    /// Installs a published appearance's model now rather than at the rig's next tick.
    pub fn sync_skin_model(
        &mut self,
        runtime_id: u64,
        source: Option<&Arc<render_api::SkinGeometrySource>>,
    ) {
        let Some(lifetime) = self.runtime_to_lifetime.get(&runtime_id) else {
            return;
        };
        let model = |state: &ActorRigState| {
            state
                .skin_skeleton()
                .map(|skeleton| Arc::as_ptr(&skeleton.prepared))
        };
        let preparation = &self.skin_preparation;
        let (next_reset, next_rest) = (self.next_reset_generation, self.next_rest_reset_generation);
        // Re-syncing the published model every frame, or a model still being prepared, keeps
        // the rig's revision.
        let Some((rejected, remodeled)) = self.rigs.update(lifetime, |state| {
            let (before, identity) = (model(state), skin::skin_identity(state));
            let rejected = skin::sync_skin(state, source, preparation);
            let remodeled = model(state) != before;
            // Tick evaluation settles a skeleton reset's generations; between ticks they
            // advance here, so pose and rest caches keyed on them rebuild for the new bones.
            if remodeled {
                state.reset_generation = next_reset;
                state.rest_reset_generation = next_rest;
            }
            (
                remodeled || skin::skin_identity(state) != identity,
                (rejected, remodeled),
            )
        }) else {
            return;
        };
        if rejected {
            self.stats.invalid_skin_geometries =
                self.stats.invalid_skin_geometries.saturating_add(1);
        }
        if remodeled {
            self.next_reset_generation = self.next_reset_generation.saturating_add(1);
            self.next_rest_reset_generation = self.next_rest_reset_generation.saturating_add(1);
        }
    }

    /// Prepares the standard skin models on the worker as soon as the catalog is available.
    pub(super) fn prewarm_skin_catalog(&mut self) {
        if let Some(assets) = &self.assets {
            self.skin_preparation.prewarm_catalog(assets);
        }
    }

    /// Dispatches one bounded batch through the existing Rayon pool, beside any still running.
    pub fn submit_skin_preparation(&mut self) {
        if let Some(assets) = &self.assets {
            self.skin_preparation.submit(assets);
        }
    }

    /// Tests can await their own submitted batch without making a timing assertion.
    #[cfg(any(test, feature = "test-support"))]
    pub fn finish_skin_fixture_batch(&mut self) {
        self.skin_preparation.finish_fixture_batch();
    }

    pub fn skin_preparation_pending(&self) -> bool {
        self.skin_preparation.is_pending()
    }
}
