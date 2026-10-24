//! Render-target validation for actor material pipelines.
//!
//! An authored material can ask for state the adapter's targets cannot represent, such as
//! blending into a format that is not blendable there or a sample count the format does not
//! support, and a pipeline can still fail to compile. Either way the material draws with its
//! view's unauthored pipeline, is counted once, and never stalls readiness or the frame.

use std::{collections::HashSet, sync::Arc};

use bevy::render::render_resource::{
    CachedPipelineState, CachedRenderPipelineId, PipelineCache, TextureFormat,
    TextureFormatFeatureFlags, TextureFormatFeatures,
};

use super::{ActorPipeline, ActorPipelineContract, ActorPipelineKey};

/// The adapter's features for one render-target format.
pub(crate) type FormatFeatures = Arc<dyn Fn(TextureFormat) -> TextureFormatFeatures + Send + Sync>;

/// Which contracts the targets can represent, and the contracts that fell back.
#[derive(Default)]
pub(crate) struct TargetValidation {
    features: Option<FormatFeatures>,
    fallen_back: HashSet<ActorPipelineContract>,
    /// Requested authored pipelines not yet seen compiled, with the key that requested them.
    pending: Vec<(ActorPipelineKey, CachedRenderPipelineId)>,
}

impl TargetValidation {
    /// Installs the adapter's format features; until then every contract is representable.
    pub(crate) fn set_features(&mut self, features: FormatFeatures) {
        self.features = Some(features);
    }

    /// Whether the adapter has format features installed.
    pub(crate) fn has_features(&self) -> bool {
        self.features.is_some()
    }

    /// Distinct material contracts drawn with the unauthored pipeline instead.
    #[cfg(test)]
    pub(crate) fn fallback_count(&self) -> usize {
        self.fallen_back.len()
    }

    /// Whether the targets can draw `contract`: its colour format blends when it blends and
    /// supports its sample count, and alpha to coverage has samples to cover.
    fn representable(&self, contract: &ActorPipelineContract) -> bool {
        let samples = contract.msaa.samples();
        if contract.alpha_to_coverage && samples < 2 {
            return false;
        }
        let Some(features) = &self.features else {
            return true;
        };
        let flags = features(contract.format).flags;
        (contract.raster.blend.is_none() || flags.contains(TextureFormatFeatureFlags::BLENDABLE))
            && flags.sample_count_supported(samples)
    }

    /// Records a fallback, logging it only the first time.
    fn fall_back(&mut self, contract: ActorPipelineContract, reason: &str) {
        if self.fallen_back.insert(contract) {
            bevy::log::warn!(
                reason,
                fallbacks = self.fallen_back.len(),
                "an actor material draws with the unauthored pipeline"
            );
        }
    }
}

impl ActorPipelineKey {
    /// The view's unauthored draw, which every view can represent.
    fn fallback(self) -> Self {
        Self {
            material: assets::EntityRenderMaterial::Default as u32,
            raster: assets::entity::material::UNAUTHORED_RENDER_STATE,
            ..self
        }
    }
}

impl ActorPipeline {
    /// The draw pipeline of `key`, requesting it once, or the view's unauthored pipeline when
    /// the targets cannot represent it.
    pub(super) fn request_draw(
        &mut self,
        cache: &PipelineCache,
        key: ActorPipelineKey,
    ) -> Option<CachedRenderPipelineId> {
        let contract = key.contract();
        if let Some(id) = self.draw_variants.get(&contract) {
            return Some(*id);
        }
        let fallback = key.fallback();
        let id = if fallback.contract() == contract {
            self.variants.specialize(cache, key).ok()?
        } else if self.targets.representable(&contract) {
            let id = self.variants.specialize(cache, key).ok()?;
            self.targets.pending.push((key, id));
            id
        } else {
            let id = self.request_draw(cache, fallback)?;
            self.targets
                .fall_back(contract, "unrepresentable render target state");
            id
        };
        self.draw_variants.insert(contract, id);
        Some(id)
    }

    /// The motion pipeline of `key`, or of the view's unauthored draw when the targets cannot
    /// represent `key`. The unauthored draw itself is always requested, as in
    /// [`Self::request_draw`], so the fallback ends there.
    pub(super) fn request_motion(
        &mut self,
        cache: &PipelineCache,
        key: ActorPipelineKey,
    ) -> Option<CachedRenderPipelineId> {
        let contract = key.contract();
        if let Some(id) = self.motion_ids.get(&contract) {
            return Some(*id);
        }
        let fallback = key.fallback();
        let id = if fallback.contract() == contract || self.targets.representable(&contract) {
            self.motion_variants.specialize(cache, key).ok()?
        } else {
            self.request_motion(cache, fallback)?
        };
        self.motion_ids.insert(contract, id);
        Some(id)
    }

    /// Redirects authored draws whose pipelines failed to compile to their view's unauthored
    /// pipeline, in draws and in readiness, so a failed material neither disappears nor stalls
    /// loading.
    pub(super) fn settle_failures(
        &mut self,
        cache: &PipelineCache,
        mut failed: impl FnMut(CachedRenderPipelineId) -> Option<bool>,
    ) {
        let pending = std::mem::take(&mut self.targets.pending);
        for (key, id) in pending {
            match failed(id) {
                None => self.targets.pending.push((key, id)),
                Some(false) => {}
                Some(true) => {
                    let Some(fallback) = self.request_draw(cache, key.fallback()) else {
                        self.targets.pending.push((key, id));
                        continue;
                    };
                    let contract = key.contract();
                    self.draw_variants.insert(contract, fallback);
                    for ids in self
                        .prewarmed
                        .values_mut()
                        .chain(self.session_prewarmed.values_mut())
                    {
                        for entry in ids.iter_mut().filter(|entry| **entry == id) {
                            *entry = fallback;
                        }
                    }
                    self.targets
                        .fall_back(contract, "pipeline failed to compile");
                }
            }
        }
    }
}

/// Whether `id` compiled (`Some(false)`), failed (`Some(true)`) or is still pending (`None`).
pub(super) fn compile_outcome(cache: &PipelineCache, id: CachedRenderPipelineId) -> Option<bool> {
    match cache.get_render_pipeline_state(id) {
        CachedPipelineState::Ok(_) => Some(false),
        CachedPipelineState::Err(_) => Some(true),
        _ => None,
    }
}

#[cfg(test)]
#[path = "targets_tests.rs"]
mod tests;
