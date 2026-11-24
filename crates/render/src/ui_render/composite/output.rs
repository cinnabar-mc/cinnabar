//! The final per-view UI pass: the deferred layer composited straight into the camera output,
//! or Bevy's output blit when nothing wrote it.
use super::*;

/// Present when [`ui_present`] replaced the output blit, so the final composite may wait for it.
#[derive(Resource)]
pub(crate) struct UiPresentInstalled;

/// Attachments and native composition state needed by the final per-view presentation pass.
type UiPresentQuery = (
    &'static ViewTarget,
    &'static bevy::core_pipeline::upscaling::ViewUpscalingPipeline,
    Option<&'static ExtractedCamera>,
    Option<&'static UiLayerTexture>,
    Option<&'static NativeUiTarget>,
);

/// Presents native screen UI after the world upscale, with or without a final alpha layer.
pub(crate) fn ui_present(
    world: &World,
    view: bevy::render::renderer::ViewQuery<UiPresentQuery>,
    mut context: RenderContext,
) {
    let entity = view.entity();
    let (output, _, camera, layer, native) = view.into_inner();
    if let Some(layer) = layer {
        layer.presented.store(false, Ordering::Relaxed);
        layer.content_presented.store(false, Ordering::Relaxed);
        layer.composited.store(false, Ordering::Relaxed);
    }
    let native = native.filter(|native| native.is_ready());
    let target = UiTarget::new(output, native);
    let resources = world
        .get_resource::<UiGpu>()
        .and_then(|gpu| gpu.composite_pipelines.get(&entity).copied())
        .zip(world.get_resource::<PipelineCache>())
        .zip(world.get_resource::<UiCompositePipeline>());
    if let Some(layer) = layer.filter(|layer| layer.present.load(Ordering::Relaxed))
        && let Some(((pipelines, cache), composite_pipeline)) = resources
    {
        let layout = cache.get_bind_group_layout(&composite_pipeline.layout);
        if layer.direct_output
            && let Some(pipeline) = pipelines
                .output
                .and_then(|id| cache.get_render_pipeline(id))
        {
            let clear = match camera.map(|camera| &camera.output_mode) {
                Some(CameraOutputMode::Write { clear_color, .. }) => *clear_color,
                _ => ClearColorConfig::Default,
            };
            let clear = match clear {
                ClearColorConfig::Default => Some(world.resource::<ClearColor>().0.into()),
                ClearColorConfig::Custom(color) => Some(color.into()),
                ClearColorConfig::None => None,
            };
            if let Some(mut attachment) = target.out_texture_color_attachment(clear) {
                // A viewport entirely outside the output draws nothing.
                let scissor = match camera.and_then(|camera| camera.viewport.as_ref()) {
                    Some(viewport) => composite_scissor(
                        (viewport.physical_position, viewport.physical_size),
                        crate::render_bounds::extent(attachment.view),
                    )
                    .map(Some),
                    None => Some(None),
                };
                if let Some(scissor) = scissor {
                    attachment.ops.load = composite_load(attachment.ops.load, scissor);
                    encode_composite(
                        &mut context,
                        world,
                        CompositeSources {
                            layer,
                            scene: target.main_texture_view(),
                            target,
                        },
                        attachment,
                        scissor,
                        pipeline,
                        &layout,
                    );
                }
                layer.presented.store(true, Ordering::Relaxed);
                if let Some(native) = native {
                    native.mark_presented();
                }
                return;
            }
        }
        if let Some(pipeline) = cache.get_render_pipeline(pipelines.main) {
            composite(&mut context, world, target, layer, pipeline, &layout);
            layer.composited.store(true, Ordering::Relaxed);
        }
    }
    if let Some(native) = native {
        native.present(world, &mut context, output);
    }
}

type UiOutputQuery = (
    &'static ViewTarget,
    &'static bevy::core_pipeline::upscaling::ViewUpscalingPipeline,
    Option<&'static ExtractedCamera>,
    Option<&'static UiLayerTexture>,
    Option<&'static bevy::render::view::ResolvedCompositingSpace>,
    Option<&'static NativeUiTarget>,
);

/// Qualifies content only after direct output or a successfully encoded ordinary output blit.
fn observe_ui_output(
    view: bevy::render::renderer::ViewQuery<UiOutputQuery>,
    cache: Res<PipelineCache>,
    blit: Res<bevy::core_pipeline::blit::BlitPipeline>,
    mut pipelines: ResMut<
        bevy::render::render_resource::SpecializedRenderPipelines<
            bevy::core_pipeline::blit::BlitPipeline,
        >,
    >,
) {
    let (target, _, camera, layer, space, native) = view.into_inner();
    let Some(layer) = layer else {
        return;
    };
    if !layer.complete_content() {
        return;
    }
    if layer.presented.load(Ordering::Relaxed) {
        layer.content_presented.store(true, Ordering::Relaxed);
        return;
    }
    if layer.present.load(Ordering::Relaxed) && !layer.composited.load(Ordering::Relaxed) {
        return;
    }
    if let Some(native) = native.filter(|native| native.is_ready()) {
        layer
            .content_presented
            .store(native.was_presented(), Ordering::Relaxed);
        return;
    }
    let blend_state = match camera.map(|camera| &camera.output_mode) {
        Some(CameraOutputMode::Skip) => return,
        Some(CameraOutputMode::Write { blend_state, .. }) => blend_state.or_else(|| {
            camera
                .filter(|camera| camera.sorted_camera_index_for_target > 0)
                .map(|_| BlendState::ALPHA_BLENDING)
        }),
        None => None,
    };
    let Some(target_format) = target.out_texture_view_format() else {
        return;
    };
    let pipeline = pipelines.specialize(
        &cache,
        &blit,
        bevy::core_pipeline::blit::BlitPipelineKey {
            target_format,
            blend_state,
            samples: 1,
            source_space: bevy::render::view::ResolvedCompositingSpace::space(space),
        },
    );
    layer.content_presented.store(
        cache.get_render_pipeline(pipeline).is_some(),
        Ordering::Relaxed,
    );
}

/// Runs Bevy's output blit only while neither native composition nor the HUD wrote the output.
fn needs_output_blit(
    view: bevy::render::renderer::ViewQuery<(Option<&UiLayerTexture>, Option<&NativeUiTarget>)>,
) -> bool {
    let (layer, native) = view.into_inner();
    !layer.is_some_and(|layer| layer.presented.load(Ordering::Relaxed))
        && !native.is_some_and(NativeUiTarget::was_presented)
}

/// Presents the HUD after post-processing, then blits only if direct output was unavailable.
pub(crate) fn install_present_node(world: &mut World) {
    if world.contains_resource::<UiPresentInstalled>() {
        return;
    }
    let installed = world
        .try_schedule_scope(Core3d, |world, schedule| {
            crate::scene_target::order_camera_stages(schedule);
            schedule
                .remove_systems_in_set(
                    upscaling,
                    world,
                    bevy::ecs::schedule::ScheduleCleanupPolicy::RemoveSystemsOnly,
                )
                .expect("replace the output blit");
            schedule.add_systems((
                crate::gpu_timing::profiled(
                    ui_present,
                    (!cfg!(target_os = "macos")).then_some(render_scene::RuntimeStage::GpuBlit),
                    "UiPresent",
                )
                .after(Core3dSystems::PostProcess)
                .after(super::overlay::UiOverlayLabel)
                .after(super::overlay::UiOverlayPostLabel),
                crate::gpu_timing::profiled(
                    upscaling,
                    (!cfg!(target_os = "macos")).then_some(render_scene::RuntimeStage::GpuBlit),
                    "Upscaling",
                )
                .after(ui_present)
                .after(Core3dSystems::PostProcess)
                .run_if(needs_output_blit),
                observe_ui_output.after(ui_present).after(upscaling),
            ));
        })
        .is_ok();
    if installed {
        world.insert_resource(UiPresentInstalled);
    }
}
