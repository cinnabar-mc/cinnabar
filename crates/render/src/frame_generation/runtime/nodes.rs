//! Real-frame foreground draws are retained independently of the interpolated world.
use super::*;
use bevy::{
    camera::MainPassResolutionOverride,
    core_pipeline::{Core3d, Core3dSystems},
    render::{
        camera::ExtractedCamera,
        view::{ExtractedView, ViewDepthStencilTexture},
    },
};

#[derive(Resource)]
struct Installed;

/// Installs colour clears and deferred sharp names once the camera schedule exists.
pub(super) fn install(world: &mut World) {
    if world.contains_resource::<Installed>() {
        return;
    }
    if world
        .try_schedule_scope(Core3d, |_, schedule| {
            schedule.add_systems((
                clear.in_set(Core3dSystems::Prepass),
                labels
                    .in_set(Core3dSystems::MainPass)
                    .after(crate::scene_target::ScenePass::Transparent)
                    .before(crate::ui_render::UiWorldLabel),
            ));
        })
        .is_ok()
    {
        world.insert_resource(Installed);
    }
}

/// Clears both current-frame layers even when neither hand nor labels has a draw.
fn clear(
    world: &World,
    query: bevy::render::renderer::ViewQuery<&ViewState, With<FrameGenerationActive>>,
    mut context: RenderContext,
) {
    let state = query.into_inner();
    let attachments = [&state.0.hand, &state.0.labels].map(|layer| {
        let mut attachment = layer.attachment();
        attachment.ops.load = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        Some(attachment)
    });
    context
        .command_encoder()
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear current foreground layers"),
            color_attachments: &attachments,
            ..Default::default()
        });
    let _ = world;
}

type Labels = (
    &'static ViewState,
    &'static ExtractedCamera,
    &'static ExtractedView,
    &'static ViewDepthStencilTexture,
    Option<&'static MainPassResolutionOverride>,
);

/// Uses actual current world depth and the unchanged label draw function, before the hand.
fn labels(
    world: &World,
    query: bevy::render::renderer::ViewQuery<Labels, With<FrameGenerationActive>>,
    mut context: RenderContext,
) -> Result {
    let entity = query.entity();
    let (state, camera, view, depth, resolution) = query.into_inner();
    crate::nametag_render::overlay::encode_labels(
        world,
        entity,
        camera,
        resolution,
        view,
        depth,
        state.0.labels.attachment(),
        &mut context,
    )
    .map(|_| ())
}
