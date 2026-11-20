use super::SceneTarget;
use crate::render_counting::CountedTrackedPass as _;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::core_3d::{AlphaMask3d, Opaque3d},
    prelude::*,
    render::{
        camera::ExtractedCamera,
        render_phase::ViewBinnedRenderPhases,
        render_resource::{RenderPassDescriptor, StoreOp},
        renderer::RenderContext,
        view::{ExtractedView, ViewDepthStencilTexture, ViewTarget},
    },
};

type SceneOpaqueQuery = (
    &'static ExtractedCamera,
    &'static ExtractedView,
    &'static ViewTarget,
    Option<&'static SceneTarget>,
    &'static ViewDepthStencilTexture,
    Option<&'static MainPassResolutionOverride>,
    Option<&'static bevy::core_pipeline::skybox::SkyboxPipelineId>,
    Option<&'static bevy::core_pipeline::skybox::SkyboxBindGroup>,
    Option<&'static bevy::render::view::ViewUniformOffset>,
);

/// Draws the sky when this pass hosts it, then opaque and cutout geometry, retaining the
/// samples for later world passes.
pub(crate) fn scene_opaque<const AFTER_PACK: bool>(
    world: &World,
    query: bevy::render::renderer::ViewQuery<SceneOpaqueQuery>,
    mut context: RenderContext,
) {
    let entity = query.entity();
    if super::unavailable(world, entity) {
        return;
    }
    let draw_native = crate::shaderpack::native_pass_enabled(world, entity, AFTER_PACK);
    if AFTER_PACK && !draw_native {
        return;
    }
    let (camera, view, target, scene, depth, resolution, sky_pipeline, sky_group, offset) =
        query.into_inner();
    use crate::atmosphere_render::sky_pass;
    let draws_sky =
        !AFTER_PACK && draw_native && sky_pass::host(world, entity) == sky_pass::SkyHost::Opaque;
    let phases = world
        .get_resource::<ViewBinnedRenderPhases<Opaque3d>>()
        .zip(world.get_resource::<ViewBinnedRenderPhases<AlphaMask3d>>())
        .and_then(|(opaque, cutout)| {
            opaque
                .get(&view.retained_view_entity)
                .zip(cutout.get(&view.retained_view_entity))
        });
    if phases.is_none() && !draws_sky {
        return;
    }
    let color = scene.map_or_else(
        || target.get_color_attachment(),
        |scene| scene.color_attachment(target, false),
    );
    let depth = depth.get_attachment(StoreOp::Store);
    let viewport = Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution).map(
        |viewport| {
            crate::render_bounds::viewport(&viewport, crate::render_bounds::extent(color.view))
        },
    );
    let sky = scene.is_none() && (sky_pipeline.is_some() || sky_group.is_some());
    if draw_native
        && let Some((opaque, cutout)) = phases
        && crate::gpu_timing::draw_categories(
            world,
            &mut context,
            entity,
            opaque,
            cutout,
            sky,
            color.clone(),
            depth.clone(),
            viewport.clone(),
        )
    {
        return;
    }
    let diagnostics = context.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let colors = [Some(color)];
    let mut pass = context.counted_tracked_render_pass(RenderPassDescriptor {
        label: Some("main opaque and cutout scene"),
        color_attachments: &colors,
        depth_stencil_attachment: Some(depth),
        timestamp_writes: crate::gpu_timing::render_pass_timestamps(
            world,
            render_scene::RuntimeStage::GpuOpaque,
        ),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    let span = diagnostics.pass_span(&mut pass, "main_opaque_pass_3d");
    if !draw_native {
        span.end(&mut pass);
        return;
    }
    match &viewport {
        Some(Some(viewport)) => pass.set_camera_viewport(viewport),
        Some(None) => {
            span.end(&mut pass);
            return;
        }
        None => {}
    }
    if draws_sky {
        sky_pass::draw(world, entity, &mut pass);
    }
    let Some((opaque, cutout)) = phases else {
        span.end(&mut pass);
        return;
    };
    if !opaque.is_empty()
        && let Err(error) = opaque.render(&mut pass, world, entity)
    {
        bevy::log::error!("Error rendering the opaque scene: {error:?}");
    }
    if !cutout.is_empty()
        && let Err(error) = cutout.render(&mut pass, world, entity)
    {
        bevy::log::error!("Error rendering the cutout scene: {error:?}");
    }
    if scene.is_none()
        && let (Some(pipeline), Some(group), Some(offset)) = (sky_pipeline, sky_group, offset)
        && let Some(pipeline) = world
            .resource::<bevy::render::render_resource::PipelineCache>()
            .get_render_pipeline(pipeline.0)
    {
        pass.set_render_pipeline(pipeline);
        pass.set_bind_group(0, &group.0.0, &[offset.offset, group.0.1]);
        pass.draw(0..3, 0..1);
    }
    span.end(&mut pass);
}

type SceneFinishQuery = (&'static ViewTarget, &'static SceneTarget);

/// Resolves the final scene samples before single-sample post-processing.
pub(crate) fn scene_finish(
    world: &World,
    query: bevy::render::renderer::ViewQuery<SceneFinishQuery>,
    mut context: RenderContext,
) -> bevy::ecs::error::Result {
    let (target, scene) = query.into_inner();
    let context = &mut context;
    scene.finish(context, world, target);
    Ok(())
}
