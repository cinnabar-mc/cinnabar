//! The opaque pass skips clearing scene colour when its own draws define every sample.
//!
//! Depth starts at the far plane, so the full-screen stock sky, drawn at the far plane with a
//! `GreaterEqual` test, writes every sample that no geometry wrote depth to. Drawn first, it
//! writes all of them. With opaque alpha it writes what the clear would have (the clear's alpha
//! is required to be 1), and geometry that replaces whole samples defines the rest, so the clear
//! is never observed.

use bevy::{
    core_pipeline::core_3d::{AlphaMask3d, Opaque3d},
    prelude::{Entity, World},
    render::{
        render_phase::{BinnedPhaseItem, BinnedRenderPhase},
        render_resource::{
            CachedPipeline, CachedRenderPipelineId, ColorWrites, LoadOp, PipelineCache,
            PipelineDescriptor,
        },
    },
};

/// The colour and depth loads a pass begins with, and how its draws cover the target.
pub(super) struct OpaqueSkyPass<'a> {
    pub(super) colour: &'a LoadOp<wgpu::Color>,
    pub(super) depth: &'a LoadOp<f32>,
    /// No camera viewport or resolution override narrows the draws.
    pub(super) full_target: bool,
    pub(super) sky_first: bool,
    pub(super) phases: Option<(
        &'a BinnedRenderPhase<Opaque3d>,
        &'a BinnedRenderPhase<AlphaMask3d>,
    )>,
}

/// Whether `pass` may begin without clearing colour, provided it then draws the sky with
/// opaque alpha through `sky_pass::draw_with_alpha`.
pub(super) fn skips_clear(world: &World, view: Entity, pass: &OpaqueSkyPass<'_>) -> bool {
    let LoadOp::Clear(colour) = pass.colour else {
        return false;
    };
    if colour.a != 1.0
        || *pass.depth != LoadOp::Clear(0.0)
        || !pass.full_target
        || crate::strict_webgpu::enabled()
        || !crate::atmosphere_render::sky_pass::covers_with_opaque_alpha(world, view)
    {
        return false;
    }
    if pass.sky_first {
        return true;
    }
    let cache = world.resource::<PipelineCache>();
    pass.phases.is_none_or(|(opaque, cutout)| {
        phase_replaces_samples(opaque, cache, |key| key.pipeline)
            && phase_replaces_samples(cutout, cache, |key| key.pipeline)
    })
}

/// Whether every batch set queued in `phase` draws with a pipeline that replaces samples.
fn phase_replaces_samples<BPI: BinnedPhaseItem>(
    phase: &BinnedRenderPhase<BPI>,
    cache: &PipelineCache,
    pipeline: impl Fn(&BPI::BatchSetKey) -> CachedRenderPipelineId,
) -> bool {
    let replaces = |key: &BPI::BatchSetKey| replaces_samples(cache, pipeline(key));
    phase.multidrawable_meshes.keys().all(replaces)
        && phase.batchable_meshes.keys().all(|(key, _)| replaces(key))
        && phase
            .unbatchable_meshes
            .keys()
            .all(|(key, _)| replaces(key))
        && phase.non_mesh_items.keys().all(|(key, _)| replaces(key))
}

/// Whether `pipeline` writes all four channels of every sample it covers without blending, so
/// those samples never show what the pass began with.
pub(crate) fn replaces_samples(cache: &PipelineCache, pipeline: CachedRenderPipelineId) -> bool {
    let Some(CachedPipeline {
        descriptor: PipelineDescriptor::RenderPipelineDescriptor(descriptor),
        ..
    }) = cache.pipelines().nth(pipeline.id())
    else {
        return false;
    };
    descriptor.fragment.as_ref().is_some_and(|fragment| {
        !fragment.targets.is_empty()
            && fragment.targets.iter().all(|target| {
                target.as_ref().is_some_and(|target| {
                    target.blend.is_none() && target.write_mask == ColorWrites::ALL
                })
            })
    })
}
