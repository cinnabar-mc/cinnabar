//! Frame selection and submission routing for terrain occlusion.

use super::*;

/// Queue-time access: chooses the GPU-culled view and records its pipelines.
#[derive(SystemParam)]
pub(in crate::chunk) struct GpuCullQueue<'w, 's> {
    culling: Option<Res<'w, render_scene::ChunkOcclusionCulling>>,
    support: Option<Res<'w, GpuCullSupport>>,
    state: Option<ResMut<'w, GpuCull>>,
    frame: Option<ResMut<'w, GpuCullFrame>>,
    feedback: Option<ResMut<'w, feedback::GpuCullFeedback>>,
    direct_support: Option<Res<'w, DirectOcclusionSupport>>,
    direct: Option<ResMut<'w, DirectOcclusionFrame>>,
    direct_state: Option<Res<'w, DirectOcclusion>>,
    adapter: Option<Res<'w, RenderAdapter>>,
    views: Query<
        'w,
        's,
        (
            Option<&'static Frustum>,
            Option<&'static RenderLayers>,
            Option<&'static ExtractedView>,
            Option<&'static Msaa>,
        ),
    >,
}

impl GpuCullQueue<'_, '_> {
    /// The experiment keeps all colour streams in the ordinary CPU command order.
    pub(in crate::chunk) fn raster_enabled(&self) -> bool {
        self.state
            .as_deref()
            .is_some_and(|state| state.raster_enabled)
    }

    /// Captures the same current allocations and stream orders as uncullled indirect drawing.
    pub(in crate::chunk) fn set_raster_orders(
        &mut self,
        orders: [&[Entity]; 2],
        allocations: &Query<&GpuChunkAllocation>,
    ) {
        if let Some(state) = self.state.as_deref_mut() {
            for (target, entities) in state.raster_orders.iter_mut().zip(orders) {
                target.clear();
                target.extend(entities.iter().filter_map(|entity| {
                    allocations
                        .get(*entity)
                        .ok()
                        .map(|allocation| allocation.metadata_index)
                }));
            }
        }
    }

    /// Reuses the solid program with colour writes disabled for the early depth subset.
    pub(in crate::chunk) fn raster_depth_pipeline(
        &mut self,
        cache: &PipelineCache,
        source: CachedRenderPipelineId,
    ) -> CachedRenderPipelineId {
        let state = self.state.as_deref_mut().expect("selected raster state");
        *state.depth_pipelines.entry(source).or_insert_with(|| {
            let mut descriptor = cache.get_render_pipeline_descriptor(source).clone();
            descriptor.label = Some("terrain raster occluder depth".into());
            for target in descriptor
                .fragment
                .as_mut()
                .expect("solid fragment")
                .targets
                .iter_mut()
                .flatten()
            {
                target.write_mask = wgpu::ColorWrites::empty();
            }
            cache.queue_render_pipeline(descriptor)
        })
    }

    /// Missing preferences retain the renderer's default behavior in isolated consumers.
    fn enabled(&self) -> bool {
        self.culling
            .as_deref()
            .map_or(render_api::DEFAULT_CHUNK_OCCLUSION_CULLING, |preference| {
                preference.0
            })
    }

    /// The view to cull on the GPU this frame; probes keep every view on the CPU path.
    pub(in crate::chunk) fn select<'a>(
        &mut self,
        draw_mode: ChunkDrawMode,
        probing: bool,
        candidates: impl IntoIterator<Item = (Entity, &'a MainEntity, &'a ExtractedView, bool)>,
    ) -> Option<Entity> {
        if let Some(frame) = self.frame.as_deref_mut() {
            frame.view = None;
        }
        let supported = self.support.as_deref().is_some_and(|support| support.0)
            && self.state.as_deref().is_some_and(|state| state.ready);
        if probing || !supported || !self.enabled() || draw_mode != ChunkDrawMode::MultiDrawIndirect
        {
            if let Some(feedback) = self.feedback.as_deref_mut() {
                feedback.select(None, None);
            }
            return None;
        }
        let view = select_gpu_cull_view(candidates, |entity| {
            self.views
                .get(entity)
                .is_ok_and(|(frustum, layers, ..)| gpu_cull_view_eligible(frustum, layers))
        });
        let Some(feedback) = self.feedback.as_deref_mut() else {
            return view;
        };
        let context = view.filter(|_| feedback.calibrated()).and_then(|entity| {
            let (_, _, extracted, msaa) = self.views.get(entity).ok()?;
            Some(work_admission::WorkContext::of(
                &self.adapter.as_deref()?.get_info(),
                extracted?,
                *msaa?,
            ))
        });
        let admitted = feedback.select(view, context);
        view.filter(|_| admitted)
    }

    pub(in crate::chunk) fn set_view(&mut self, view: GpuCullView) {
        if let Some(frame) = self.frame.as_deref_mut() {
            frame.view = Some(view);
        }
    }

    /// The direct-drawn view whose terrain occlusion is tested; probes keep it off.
    pub(in crate::chunk) fn select_direct<'a>(
        &mut self,
        draw_mode: ChunkDrawMode,
        probing: bool,
        candidates: impl IntoIterator<Item = (Entity, &'a MainEntity, &'a ExtractedView, bool)>,
    ) -> Option<Entity> {
        self.direct.as_deref_mut()?.clear();
        let supported = self
            .direct_support
            .as_deref()
            .is_some_and(|support| support.0)
            && self
                .direct_state
                .as_deref()
                .is_some_and(|state| state.ready);
        if probing || !supported || !self.enabled() || draw_mode != ChunkDrawMode::Direct {
            return None;
        }
        select_gpu_cull_view(candidates, |entity| {
            self.views
                .get(entity)
                .is_ok_and(|(frustum, layers, ..)| gpu_cull_view_eligible(frustum, layers))
        })
    }

    /// Starts the selected direct view's frame; `Some(true)` routes its solid terrain to the
    /// terrain pass.
    pub(in crate::chunk) fn begin_direct(
        &mut self,
        entity: Entity,
        view: &ExtractedView,
        msaa: Msaa,
        solid: (CachedRenderPipelineId, DrawFunctionId),
    ) -> Option<bool> {
        let wants = self
            .direct_state
            .as_deref()
            .is_some_and(|state| state.wants_verdict(entity, view, msaa));
        let same_frame = self.direct_state.as_deref().is_some_and(|s| s.same_frame());
        let routed = self
            .direct
            .as_deref_mut()?
            .begin(entity, view, solid, wants);
        if same_frame {
            self.direct.as_deref_mut()?.force_depth_pass();
        }
        Some(routed && !same_frame)
    }

    /// The selected direct view's frame; `None` when the path is not installed.
    pub(in crate::chunk) fn direct_frame(&mut self) -> Option<&mut DirectOcclusionFrame> {
        self.direct.as_deref_mut()
    }
}
