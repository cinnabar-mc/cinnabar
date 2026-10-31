//! One finite worker per camera; pending compiles and resizes leave real rendering available.
use super::*;
use bevy::{
    camera::{CameraOutputMode, NormalizedRenderTarget},
    render::{
        camera::ExtractedCamera,
        view::{ExtractedView, ViewDepthStencilTexture},
    },
};
use std::{collections::HashMap, thread::JoinHandle};

#[derive(Default)]
pub(super) struct Jobs {
    pending: HashMap<Entity, (gpu::Key, JoinHandle<Option<gpu::Ready>>)>,
    failed: HashMap<Entity, gpu::Key>,
}

type Views<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static FrameGenerationCamera,
        &'static ExtractedCamera,
        &'static ExtractedView,
        &'static ViewTarget,
        &'static ViewDepthStencilTexture,
        &'static Msaa,
        Option<&'static ViewState>,
        Option<&'static crate::ui_render::target::NativeUiTarget>,
    ),
>;

type Unsupported = Or<(
    With<render_scene::EnhancedRendering>,
    With<render_scene::shaderpack::ShaderPackRendering>,
)>;

/// Admits complete opaque window views and publishes only matching finished preparations.
#[allow(clippy::too_many_arguments)]
pub(super) fn views(
    mut commands: Commands,
    device: Res<RenderDevice>,
    views: Views,
    removed: Query<Entity, (With<ViewState>, Without<FrameGenerationCamera>)>,
    all_cameras: Query<&ExtractedCamera>,
    unsupported: Query<Entity, Unsupported>,
    mut jobs: Local<Jobs>,
    overlays: Option<Res<render_scene::ScreenOverlayScene>>,
    ui: Option<Res<crate::ui_render::UiGpu>>,
    cache: Res<bevy::render::render_resource::PipelineCache>,
    sharpen: Option<Res<crate::render_scale::RenderScaleSharpen>>,
) {
    for entity in &removed {
        commands
            .entity(entity)
            .remove::<(ViewState, FrameGenerationActive)>();
    }
    jobs.pending
        .retain(|entity, (_, job)| views.contains(*entity) || !job.is_finished());
    jobs.failed.retain(|entity, _| views.contains(*entity));
    let effects = overlays.is_some_and(|scene| !scene.layers.is_empty())
        || ui.is_some_and(|gpu| gpu.batches.iter().any(|batch| batch.world_projection != 0));
    for (entity, request, camera, view, target, depth, msaa, state, native) in &views {
        commands.entity(entity).remove::<FrameGenerationActive>();
        let Some(extent) = target
            .out_texture()
            .map(|view| crate::render_bounds::extent(view))
        else {
            continue;
        };
        let compatible = !effects
            && !unsupported.contains(entity)
            && !camera.hdr
            && camera.viewport.is_none()
            && matches!(camera.target, Some(NormalizedRenderTarget::Window(_)))
            && matches!(
                camera.output_mode,
                CameraOutputMode::Write {
                    blend_state: None,
                    ..
                }
            )
            && all_cameras
                .iter()
                .filter(|other| other.target == camera.target)
                .count()
                == 1
            && target.main_texture_format() == render_scene::SCENE_COLOR_FORMAT;
        if !compatible {
            continue;
        }
        let Some(output) = target.out_texture_view_format() else {
            continue;
        };
        if !output.is_srgb() {
            continue;
        }
        let key = gpu::Key {
            device: device.wgpu_device().clone(),
            size: FrameGenerationSize {
                display: extent,
                render: [depth.texture().width(), depth.texture().height()],
            },
            samples: msaa.samples(),
            output,
            depth: crate::scene_sampling::view_depth(depth).id(),
        };
        if key.size.validate(&device.limits()).is_err() {
            continue;
        }
        if let Some(state) = state.filter(|state| state.0.key == key) {
            let pose = view.world_from_view.to_matrix();
            let clip = view.clip_from_world.map_or(view.clip_from_view, |clip| {
                (clip.as_dmat4() * pose.as_dmat4()).as_mat4()
            });
            state
                .0
                .begin_frame(crate::motion_blur::history::CameraHistory::new(
                    clip,
                    pose,
                    view.viewport,
                    request.reset_epoch,
                ));
            let sharpen = sharpen.as_ref().is_some_and(|filter| filter.0.is_some());
            if native.is_some_and(|native| native.compatible(&device, &cache, sharpen, target)) {
                commands.entity(entity).insert(FrameGenerationActive);
            }
            continue;
        }
        if jobs
            .pending
            .get(&entity)
            .is_some_and(|(_, job)| job.is_finished())
        {
            let (prepared, job) = jobs
                .pending
                .remove(&entity)
                .expect("finished frame generation worker");
            if prepared == key {
                if let Ok(Some(ready)) = job.join() {
                    commands.entity(entity).insert(ViewState(Arc::new(ready)));
                } else {
                    bevy::log::warn!(
                        ?entity,
                        "frame generation preparation failed; ordinary presentation continues"
                    );
                    jobs.failed.insert(entity, key);
                }
                continue;
            }
        }
        if !jobs.pending.contains_key(&entity) && jobs.failed.get(&entity) != Some(&key) {
            let device = device.clone();
            let request = key.clone();
            let source = crate::scene_sampling::view_depth(depth).clone();
            jobs.pending.insert(
                entity,
                (
                    key,
                    std::thread::spawn(move || gpu::Ready::new(device, request, source)),
                ),
            );
        }
    }
}
