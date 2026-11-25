//! GPU resources for personal-mod meshes; protocol debug shapes have separate ownership.
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems, extract_resource::ExtractResourcePlugin,
        render_phase::AddRenderCommand,
    },
};
use render_scene::mod_scene::retained::DrawingFrame;
pub(crate) mod clips;
mod clock;
mod gpu;
#[cfg(test)]
mod ledger_tests;
mod materials;
mod pipeline;
#[cfg(test)]
mod tests;
const SHADER: Handle<Shader> = uuid_handle!("76e342a4-19e8-4592-813f-9b195ee8a15d");

/// Installs static geometry, per-view transforms and the shared unlit pipeline.
pub(super) fn install(app: &mut App) {
    crate::pipeline_warmup::register::<pipeline::Pipeline>(app);
    crate::pipeline_warmup::register_pending::<materials::Materials>(app);
    app.add_plugins(ExtractResourcePlugin::<DrawingFrame>::default());
    app.add_systems(Update, crate::chunk::admit_depth_sampling);
    app.init_resource::<clock::AnimationClock>()
        .add_plugins(ExtractResourcePlugin::<clock::AnimationClock>::default())
        .add_systems(Last, clock::advance);
    load_internal_asset!(
        app,
        SHADER,
        "retained/drawing.wgsl",
        |source: &str, path| {
            crate::shader_safety::from_wgsl(
                materials::source(
                    &mod_render::material::module(mod_render::material::BUILTIN),
                    source,
                ),
                path,
            )
        }
    );
    load_internal_asset!(
        app,
        clips::SHADER,
        "retained/clip_mask.wgsl",
        |source: &str, path| crate::shader_safety::from_wgsl(clips::source(false, source), path)
    );
    load_internal_asset!(
        app,
        clips::MSAA_SHADER,
        "retained/clip_mask.wgsl",
        |source: &str, path| crate::shader_safety::from_wgsl(clips::source(true, source), path)
    );
    crate::device_poll::install(app.sub_app_mut(RenderApp));
    app.sub_app_mut(RenderApp)
        .init_resource::<gpu::Gpu>()
        .init_resource::<pipeline::Pipeline>()
        .init_resource::<materials::Materials>()
        .add_render_command::<bevy::core_pipeline::core_3d::Transparent3d, pipeline::Draw>();
    install_systems(app.sub_app_mut(RenderApp));
    clips::install(app.sub_app_mut(RenderApp));
    crate::chunk::blocks::install(app);
}

/// Registers drawing preparation and deferred resource release in the render schedule.
fn install_systems(app: &mut SubApp) {
    app.configure_sets(Render, RetainedSystems.run_if(has_retained_work))
        .add_systems(
            Render,
            (
                gpu::queue
                    .after(super::primitives::queue)
                    .in_set(RenderSystems::Queue),
                pipeline::bindings.in_set(RenderSystems::PrepareBindGroups),
                materials::retire.in_set(crate::device_poll::FrameSubmissions),
                gpu::retire.in_set(crate::device_poll::FrameSubmissions),
            )
                .in_set(RetainedSystems),
        );
}

/// The retained drawing systems: mesh and view preparation, bindings, clip targets and the
/// deferred release of removed storage.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct RetainedSystems;

/// Whether the retained drawing systems have anything to do: retained resources in the scene,
/// or GPU, material or clip state left to release. The condition runs before the queue, so a
/// frame that removes the last drawing still runs the release systems after it.
fn has_retained_work(
    scene: Option<Res<render_scene::ModRenderScene>>,
    gpu: Option<Res<gpu::Gpu>>,
    materials: Option<Res<materials::Materials>>,
    targets: Option<Res<clips::Targets>>,
) -> bool {
    scene.is_some_and(|scene| !scene.retained.is_empty())
        || gpu.is_some_and(|gpu| !gpu.at_rest())
        || materials.is_some_and(|materials| !materials.at_rest())
        || targets.is_some_and(|targets| !targets.at_rest())
}
