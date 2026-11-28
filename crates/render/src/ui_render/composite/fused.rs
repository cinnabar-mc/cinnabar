//! The final UI composite reading the scene's two samples instead of its resolved texture, so
//! the frame needs no separate resolve pass. Exact by construction: the samples resolve through
//! the device's own tabulated resolve ([`crate::scene_target::resolve_lut`]).

use super::*;
#[cfg(test)]
#[path = "fused_tests.rs"]
mod tests;
use bevy::render::render_resource::{
    BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, CachedRenderPipelineId,
    PipelineCache, RenderPipeline, ShaderStages, TextureSampleType, TextureViewDimension,
};

/// Fused composite pipelines per output format and each view's cached binding.
#[derive(Resource)]
pub(crate) struct FusedComposite {
    layout: BindGroupLayoutDescriptor,
    pipelines: Mutex<HashMap<TextureFormat, CachedRenderPipelineId>>,
    bindings: Mutex<HashMap<Entity, FusedBinding>>,
}

/// A view's binding of its current layer and scene samples, and whether this frame drew it.
struct FusedBinding {
    targets: [TextureViewId; 2],
    group: BindGroup,
    used: bool,
}

impl Default for FusedComposite {
    fn default() -> Self {
        let texture = |binding, multisampled| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: false },
                view_dimension: TextureViewDimension::D2,
                multisampled,
            },
            count: None,
        };
        Self {
            layout: BindGroupLayoutDescriptor::new(
                "fused UI composite layout",
                &[texture(0, false), texture(3, true), texture(4, false)],
            ),
            pipelines: Mutex::default(),
            bindings: Mutex::default(),
        }
    }
}

impl FusedComposite {
    /// The compiled fused composite writing `format`, queuing it on first use.
    pub(super) fn pipeline<'a>(
        &self,
        cache: &'a PipelineCache,
        format: TextureFormat,
    ) -> Option<&'a RenderPipeline> {
        let id = *self
            .pipelines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(format)
            .or_insert_with(|| {
                let mut descriptor = composite_pipeline_descriptor(self.layout.clone());
                descriptor.label = Some("fused gamma-space UI composite".into());
                let fragment = descriptor.fragment.as_mut().unwrap();
                fragment.entry_point = Some("fused_composite_fragment".into());
                fragment.targets[0].as_mut().unwrap().format = format;
                cache.queue_render_pipeline(descriptor)
            });
        cache.get_render_pipeline(id)
    }

    /// Composites `layer` over the unresolved `samples` into `destination`, returning the open
    /// pass so a deferred invert can draw into it.
    pub(super) fn encode<'c>(
        &self,
        context: &'c mut RenderContext,
        world: &World,
        view: Entity,
        (layer, samples, table): (&UiLayerTexture, &TextureView, &TextureView),
        destination: RenderPassColorAttachment<'_>,
        pipeline: &'c RenderPipeline,
    ) -> TrackedRenderPass<'c> {
        let cache = world.resource::<PipelineCache>();
        let binding = {
            let mut bindings = self
                .bindings
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let targets = [layer.view.id(), samples.id()];
            let create = || {
                context.render_device().create_bind_group(
                    Some("fused UI composite bindings"),
                    &cache.get_bind_group_layout(&self.layout),
                    &BindGroupEntries::with_indices(((0, &layer.view), (3, samples), (4, table))),
                )
            };
            let binding = bindings.entry(view).or_insert_with(|| FusedBinding {
                targets,
                group: create(),
                used: true,
            });
            if binding.targets != targets {
                binding.targets = targets;
                binding.group = create();
            }
            binding.used = true;
            binding.group.clone()
        };
        if let Some(profile) = world.get_resource::<super::super::profile::UiProfile>() {
            profile.record_pass(render_scene::RuntimeStage::GpuUiComposite);
            profile.record_draw(render_scene::RuntimeStage::GpuUiComposite, 0);
        }
        let attachments = [Some(destination)];
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("fused gamma-space UI composite"),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
            timestamp_writes: crate::gpu_timing::ui_pass_timestamps(
                world,
                render_scene::RuntimeStage::GpuUiComposite,
            ),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_render_pipeline(pipeline);
        // The pass outlives this cached binding's local handle; wgpu keeps it alive.
        pass.wgpu_pass().set_bind_group(0, &*binding, &[]);
        pass.draw(0..3, 0..1);
        pass
    }
}

/// Drops the bindings no view drew this frame, releasing retired layer and scene targets once
/// a view is resized, removed or stops fusing its composite.
pub(crate) fn retire_unused_bindings(fused: Res<FusedComposite>) {
    fused
        .bindings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|_, binding| std::mem::take(&mut binding.used));
}
