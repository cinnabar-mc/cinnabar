//! A leading HUD invert, such as the crosshair, drawn inside the frame's final layer composite
//! instead of in a render pass of its own over the resolved scene.
//!
//! The final composite writes the layer over the scene. Wherever the layer is empty it writes
//! the scene unchanged, so an invert whose pixels the layer never covers blends against the same
//! values in either order. Deferral therefore requires the invert's rasterized bounds to miss
//! every layered batch's bounds, measured from the accepted publication's own vertices.

use super::*;
use bevy::render::render_resource::{
    CachedRenderPipelineId, PipelineCache, RenderPipelineDescriptor,
};
use std::{collections::HashMap, sync::Mutex};

/// The deferred invert batch and the pipelines that draw it into the composite's destination.
#[derive(Clone)]
pub(crate) struct DeferredInvert {
    pub(super) index: usize,
    /// Indices hidden by the first-person hand this frame, as the ordered pass omits them.
    pub(super) skip: Option<std::ops::Range<u32>>,
    pub(super) batch: UiRenderBatch,
    pub(super) location: render_model::UiTextureLocation,
    /// The invert pipeline for the view's main texture format.
    pub(super) main: CachedRenderPipelineId,
    /// The same invert for the camera output's format, when the composite writes it directly.
    pub(super) output: Option<CachedRenderPipelineId>,
}

/// The invert batch of a plan that is one leading invert followed by one deferred layer of
/// plain 2D batches, or `None` when any other topology must keep the ordered passes.
pub(super) fn leading_invert<T: std::borrow::Borrow<UiRenderBatch>, L>(
    plan: &segments::UiPassPlan,
    batches: &[(usize, T, L)],
) -> Option<usize> {
    let [first, last] = plan.segments.as_slice() else {
        return None;
    };
    let invert = first.inverted.filter(|_| first.layered.is_empty())?;
    if !last.present || last.inverted.is_some() || last.layered.is_empty() {
        return None;
    }
    let plain = |batch: &UiRenderBatch| {
        batch.isolated_depth_scope.is_none()
            && batch.filter_group.is_none()
            && batch.world_projection == 0
            && batch.depth_test == 0
            && batch.depth_write == 0
    };
    let invert_batch = batches[invert].1.borrow();
    (plain(invert_batch)
        && !segments::invert_needs_depth(invert_batch)
        && batches[last.layered.clone()]
            .iter()
            .all(|(_, batch, _)| plain(batch.borrow())))
    .then_some(invert)
}

/// Inclusive pixel bounds `[min_x, min_y, max_x, max_y]` that a batch's triangles can touch in
/// the publication's physical viewport, clipped by its scissor; `None` when it covers nothing.
pub(super) fn batch_bounds(input: &UiRenderInput, batch: &UiRenderBatch) -> Option<[f32; 4]> {
    let first = batch.first_index as usize;
    let indices = input
        .indices
        .get(first..first + batch.index_count as usize)?;
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for &index in indices {
        let vertex = input.vertices.get(index as usize)?;
        if vertex.clip_w.is_nan() || vertex.clip_w <= 0.0 {
            return Some([
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
                f32::INFINITY,
                f32::INFINITY,
            ]);
        }
        let [x, y] = vertex.position.map(|value| value / vertex.clip_w);
        bounds = [
            bounds[0].min(x),
            bounds[1].min(y),
            bounds[2].max(x),
            bounds[3].max(y),
        ];
    }
    let scissor = batch.scissor;
    let clipped = [
        bounds[0].max(scissor.x as f32),
        bounds[1].max(scissor.y as f32),
        bounds[2].min((scissor.x + scissor.width) as f32),
        bounds[3].min((scissor.y + scissor.height) as f32),
    ];
    (clipped[0] <= clipped[2] && clipped[1] <= clipped[3]).then_some(clipped)
}

/// Whether `invert` provably touches no pixel any of `layered` touches. Bounds grow by a pixel
/// each way so that edge and sample coverage cannot meet.
pub(super) fn disjoint<'a>(
    input: &UiRenderInput,
    invert: &UiRenderBatch,
    layered: impl IntoIterator<Item = &'a UiRenderBatch>,
) -> bool {
    let Some(invert) = batch_bounds(input, invert) else {
        return true;
    };
    layered.into_iter().all(|batch| {
        batch_bounds(input, batch).is_none_or(|bounds| {
            bounds[0] > invert[2] + 1.0
                || bounds[2] < invert[0] - 1.0
                || bounds[1] > invert[3] + 1.0
                || bounds[3] < invert[1] - 1.0
        })
    })
}

/// Eight-bit sRGB colour targets whose blending decodes, blends and re-encodes the same way, so
/// an invert drawn into one matches the other byte for byte.
pub(super) fn blends_alike(main: TextureFormat, output: TextureFormat) -> bool {
    let srgb8 = |format| {
        matches!(
            format,
            TextureFormat::Rgba8UnormSrgb | TextureFormat::Bgra8UnormSrgb
        )
    };
    main == output || (srgb8(main) && srgb8(output))
}

/// Output-format variants of view invert pipelines, keyed by the main-format pipeline.
#[derive(Resource, Default)]
pub(crate) struct OutputInvertPipelines(
    Mutex<HashMap<(CachedRenderPipelineId, TextureFormat), CachedRenderPipelineId>>,
);

impl OutputInvertPipelines {
    /// The ready variant of the compiled `main` invert that targets `format`, queuing it on
    /// first use; `None` until it compiles.
    pub(super) fn ready(
        &self,
        cache: &PipelineCache,
        main: CachedRenderPipelineId,
        format: TextureFormat,
    ) -> Option<CachedRenderPipelineId> {
        cache.get_render_pipeline(main)?;
        let mut variants = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = *variants.entry((main, format)).or_insert_with(|| {
            let mut descriptor: RenderPipelineDescriptor =
                cache.get_render_pipeline_descriptor(main).clone();
            if let Some(target) = descriptor
                .fragment
                .as_mut()
                .and_then(|fragment| fragment.targets.first_mut())
                .and_then(Option::as_mut)
            {
                target.format = format;
            }
            cache.queue_render_pipeline(descriptor)
        });
        cache.get_render_pipeline(id).map(|_| id)
    }
}

/// Holds at most one deferred invert for the view's final composite.
#[derive(Default)]
pub(crate) struct DeferredInvertSlot(Mutex<Option<DeferredInvert>>);

impl DeferredInvertSlot {
    pub(super) fn set(&self, invert: Option<DeferredInvert>) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = invert;
    }

    pub(super) fn take(&self) -> Option<DeferredInvert> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

/// Draws `invert` into `pass`, whose colour target has `extent`, with `pipeline`.
pub(super) fn draw<'w>(
    pass: &mut bevy::render::render_phase::TrackedRenderPass<'w>,
    world: &'w World,
    invert: &DeferredInvert,
    pipeline: &'w bevy::render::render_resource::RenderPipeline,
    extent: [u32; 2],
) -> bool {
    let gpu = world.resource::<UiGpu>();
    let (Some(vertices), Some(indices)) = (&gpu.vertex_buffer, &gpu.index_buffer) else {
        return false;
    };
    pass.set_render_pipeline(pipeline);
    super::layer::draw_batches(
        pass,
        gpu,
        vertices,
        indices,
        None,
        extent,
        None,
        &[(invert.index, &invert.batch, invert.location)],
        invert.skip.as_ref(),
        None,
        world
            .get_resource::<super::profile::UiProfile>()
            .map(|profile| (profile, render_scene::RuntimeStage::GpuUiInvert)),
    )
}

/// Draws a postponed invert in its own pass over the main texture, as the ordered plan does
/// when no final composite follows it.
pub(super) fn own_pass(
    context: &mut bevy::render::renderer::RenderContext,
    world: &World,
    main: &bevy::render::render_resource::TextureView,
    invert: &DeferredInvert,
) -> bool {
    let Some(pipeline) = world
        .resource::<PipelineCache>()
        .get_render_pipeline(invert.main)
    else {
        return false;
    };
    let attachments = [Some(
        bevy::render::render_resource::RenderPassColorAttachment {
            view: main,
            depth_slice: None,
            resolve_target: None,
            ops: bevy::render::render_resource::Operations {
                load: bevy::render::render_resource::LoadOp::Load,
                store: bevy::render::render_resource::StoreOp::Store,
            },
        },
    )];
    let mut pass =
        context.begin_tracked_render_pass(bevy::render::render_resource::RenderPassDescriptor {
            label: Some("retained depth-free HUD invert"),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
            timestamp_writes: crate::gpu_timing::ui_pass_timestamps(
                world,
                render_scene::RuntimeStage::GpuUiInvert,
            ),
            occlusion_query_set: None,
            multiview_mask: None,
        });
    if let Some(profile) = world.get_resource::<super::profile::UiProfile>() {
        profile.record_pass(render_scene::RuntimeStage::GpuUiInvert);
    }
    draw(
        &mut pass,
        world,
        invert,
        pipeline,
        crate::render_bounds::extent(main),
    )
}

#[cfg(test)]
#[path = "deferred_invert_tests.rs"]
mod tests;
