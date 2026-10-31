//! World interpolation, independent native overlays and presentation-only surface pacing.
mod gpu;
mod nodes;
mod prepare;
mod present;
#[cfg(test)]
mod scene_tests;
#[cfg(test)]
mod tests;

use super::{COLOR_FORMAT, FrameGenerationInput, FrameGenerationSize, FrameGenerator};
use bevy::{
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        renderer::{RenderContext, RenderDevice, RenderQueue},
        view::ViewTarget,
    },
};
use render_scene::frame_generation::{FrameGenerationActive, FrameGenerationCamera};
use std::sync::Arc;

/// Complete immutable attachments whose temporal state is locked independently.
#[derive(Component, Clone)]
pub(crate) struct ViewState(Arc<gpu::Ready>);

/// Marks only the extra UI encoding; it never runs extraction or an application schedule.
#[derive(Resource, Default)]
struct Replay(Option<Entity>);

/// Reuses real-frame UI resources over a generated native world image.
#[derive(bevy::ecs::schedule::ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct GeneratedUi;

/// Installs the native optional presentation effect; saved settings leave it off.
pub struct FrameGenerationPlugin;
impl Plugin for FrameGenerationPlugin {
    /// Queues preparation without starting a worker or allocating history while disabled.
    fn build(&self, app: &mut App) {
        let Some(render) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        crate::device_poll::install(render);
        render
            .init_resource::<Replay>()
            .init_resource::<present::Presenter>()
            .add_systems(ExtractSchedule, extract)
            .add_systems(
                Render,
                (
                    prepare::views
                        .in_set(RenderSystems::PrepareResources)
                        .after(crate::ui_render::target::NativeUiPrepared)
                        .after(bevy::core_pipeline::core_3d::prepare_core_3d_depth_textures),
                    nodes::install.in_set(RenderSystems::Prepare),
                    present::hold
                        .after(crate::startup_presentation::SurfaceCaptured)
                        .before(bevy::render::renderer::render_system)
                        .in_set(RenderSystems::Render),
                    present::generate
                        .after(bevy::render::renderer::render_system)
                        .before(crate::device_poll::FrameSubmissions)
                        .in_set(RenderSystems::Render),
                    present::present
                        .after(crate::device_poll::DevicePollComplete)
                        .in_set(RenderSystems::Render),
                ),
            );
        let mut schedule = Schedule::new(GeneratedUi);
        schedule.add_systems(
            (
                crate::ui_render::overlay::ui_overlay,
                crate::ui_render::composite::ui_present,
                crate::ui_render::composite::observe_ui_output,
                flush,
            )
                .chain(),
        );
        render.add_schedule(schedule);
    }
}

type MainCameras<'w, 's> = Query<
    'w,
    's,
    (
        bevy::render::sync_world::RenderEntity,
        Option<&'static FrameGenerationCamera>,
    ),
    With<Camera3d>,
>;

/// Publishes capture requests at extraction, after screenshots have been spawned by the main app.
fn extract(
    mut commands: Commands,
    cameras: Extract<MainCameras>,
    shots: Extract<Query<(), With<bevy::render::view::screenshot::Screenshot>>>,
    mut current: Query<&mut FrameGenerationCamera>,
) {
    let capture = !shots.is_empty();
    for (entity, desired) in &cameras {
        match desired {
            Some(desired) => {
                let mut desired = *desired;
                desired.capture |= capture;
                if let Ok(mut existing) = current.get_mut(entity) {
                    if *existing != desired {
                        *existing = desired;
                    }
                } else {
                    commands.entity(entity).insert(desired);
                }
            }
            None if current.contains(entity) => {
                commands
                    .entity(entity)
                    .remove::<(FrameGenerationCamera, FrameGenerationActive)>();
            }
            None => {}
        }
    }
}

/// Submits replayed UI in the same ordered queue as the real world and interpolation passes.
fn flush(mut commands: bevy::render::renderer::FlushCommands) {
    commands.flush();
}

/// Foreground geometry goes to a transparent current-frame layer, never interpolation history.
pub(crate) fn hand_attachment(
    world: &World,
    entity: Entity,
) -> Option<bevy::render::render_resource::RenderPassColorAttachment<'_>> {
    world.get::<FrameGenerationActive>(entity)?;
    Some(world.get::<ViewState>(entity)?.0.hand.attachment())
}

/// Seeds world history before adding the unwarped hand, names and retained screen UI.
pub(crate) fn begin_ui(
    world: &World,
    entity: Entity,
    context: &mut RenderContext,
    target: crate::ui_render::target::UiTarget<'_>,
) {
    if world.get::<FrameGenerationActive>(entity).is_none() || !target.is_native() {
        return;
    }
    let Some(state) = world.get::<ViewState>(entity) else {
        return;
    };
    let ready = &state.0;
    let replay = world.resource::<Replay>().0 == Some(entity);
    if !replay {
        if let Some((_, resolved)) = &ready.hand.resolved {
            let mut attachment = ready.hand.attachment();
            attachment.resolve_target = Some(resolved);
            attachment.ops.store = wgpu::StoreOp::Discard;
            context
                .command_encoder()
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("frame generation hand resolve"),
                    color_attachments: &[Some(attachment)],
                    ..Default::default()
                });
        }
        context.command_encoder().copy_texture_to_texture(
            target.main_texture().as_image_copy(),
            ready.world.as_image_copy(),
            ready.world.size(),
        );
        ready
            .temporal
            .lock()
            .expect("frame generation history")
            .world_captured = true;
    }
    let post = target.post_process_write();
    let device = context.render_device().wgpu_device().clone();
    ready.compose(
        &device,
        context.command_encoder(),
        post.source,
        post.destination,
    );
}

/// Retains the complete real image so the ordinary frame can follow its generated midpoint.
pub(crate) fn capture_real(
    world: &World,
    entity: Entity,
    context: &mut RenderContext,
    target: crate::ui_render::target::UiTarget<'_>,
    final_layer: Option<(
        &crate::ui_render::composite::UiLayerTexture,
        &bevy::render::render_resource::RenderPipeline,
        &bevy::render::render_resource::BindGroupLayout,
    )>,
) {
    if world.get::<FrameGenerationActive>(entity).is_none()
        || world.resource::<Replay>().0.is_some()
        || !target.is_native()
    {
        return;
    }
    let Some(state) = world.get::<ViewState>(entity) else {
        return;
    };
    let ready = &state.0;
    if let Some((layer, pipeline, layout)) = final_layer {
        crate::ui_render::composite::encode_external(
            context,
            world,
            target,
            layer,
            pipeline,
            layout,
            &ready.real_view,
        );
    } else {
        let device = context.render_device().wgpu_device().clone();
        ready.blit(
            &device,
            context.command_encoder(),
            &ready.copy_native,
            target.main_texture_view(),
            &ready.real_view,
        );
    }
    ready
        .temporal
        .lock()
        .expect("frame generation history")
        .real_captured = true;
}
