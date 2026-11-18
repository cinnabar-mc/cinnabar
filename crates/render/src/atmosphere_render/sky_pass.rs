//! The sky is the first draw on scene colour, in the first pass that opens it.

use super::{AtmosphereGpu, AtmospherePipelineKey};
use bevy::prelude::IntoScheduleConfigs;
use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::{Core3d, Core3dSystems},
    prelude::{Entity, Msaa, Resource, World},
    render::{
        camera::ExtractedCamera,
        render_phase::TrackedRenderPass,
        render_resource::{PipelineCache, RenderPassDescriptor, StoreOp},
        renderer::{RenderContext, ViewQuery},
        view::{ViewDepthStencilTexture, ViewTarget, ViewUniformOffset},
    },
};

#[derive(Resource)]
struct SkyPassInstalled;

/// Places the sky's own pass, when it needs one, after prepasses and before any terrain.
pub(super) fn install(world: &mut World) {
    if world.contains_resource::<SkyPassInstalled>() {
        return;
    }
    if world
        .try_schedule_scope(Core3d, |_, schedule| {
            schedule.add_systems(
                crate::gpu_timing::profiled(
                    atmosphere,
                    Some(render_scene::RuntimeStage::GpuSky),
                    "AtmospherePass",
                )
                .after(Core3dSystems::Prepass)
                .before(Core3dSystems::MainPass),
            );
        })
        .is_ok()
    {
        world.insert_resource(SkyPassInstalled);
    }
}

type AtmosphereQuery = (
    &'static ExtractedCamera,
    &'static ViewTarget,
    Option<&'static crate::scene_target::SceneTarget>,
    &'static ViewDepthStencilTexture,
    Option<&'static MainPassResolutionOverride>,
);

/// The render pass that draws a view's sky layers this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SkyHost {
    /// The sky opens its own pass, ahead of world geometry.
    Own,
    /// The direct terrain pass, which opens the scene colour before the opaque pass.
    Terrain,
    /// The ordinary opaque scene pass.
    Opaque,
}

/// Picks the first pass that opens `view`'s scene colour, so the sky stays the first draw
/// on that attachment without paying for a render pass of its own. Opaque-category GPU
/// profiling splits the opaque pass and keeps the sky's own timed pass.
pub(crate) fn host(world: &World, view: Entity) -> SkyHost {
    if crate::chunk::terrain_pass_draws_color(world, view) {
        SkyHost::Terrain
    } else if crate::scene_target::opaque_pass_installed(world)
        && !crate::gpu_timing::splits_opaque_pass(world)
    {
        SkyHost::Opaque
    } else {
        SkyHost::Own
    }
}

/// Opens the sky's own pass only for views whose sky no scene pass hosts.
fn atmosphere(world: &World, query: ViewQuery<AtmosphereQuery>, mut context: RenderContext) {
    let view = query.entity();
    if host(world, view) != SkyHost::Own || !drawn(world, view) {
        return;
    }
    let (camera, target, scene, depth, resolution) = query.into_inner();
    let colour = scene.map_or_else(
        || target.get_color_attachment(),
        |scene| scene.color_attachment(target, false),
    );
    let extent = crate::render_bounds::extent(colour.view);
    let viewport = Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution)
        .map(|viewport| crate::render_bounds::viewport(&viewport, extent));
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("sky before world geometry"),
        color_attachments: &[Some(colour)],
        depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
        timestamp_writes: crate::gpu_timing::render_pass_timestamps(
            world,
            render_scene::RuntimeStage::GpuSky,
        ),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    match viewport {
        Some(Some(viewport)) => pass.set_camera_viewport(&viewport),
        Some(None) => return,
        None => {}
    }
    draw(world, view, &mut pass);
}

/// Whether `view` shows a sky this frame: not while the panorama hides the game, nor under a
/// shader pack that draws the view's sky and world itself.
fn drawn(world: &World, view: Entity) -> bool {
    !crate::scene_target::unavailable(world, view)
        && !world
            .get_resource::<render_scene::PanoramaScene>()
            .is_some_and(|scene| !scene.game_visible())
        && !crate::shaderpack::plugin::draws_world(world, view)
}

/// Draws the stock sky, stars and finite pack cube into `pass`, which targets `view`'s scene
/// colour and depth with its viewport set, before any world geometry can cover them.
pub(crate) fn draw<'w>(world: &'w World, view: Entity, pass: &mut TrackedRenderPass<'w>) {
    if !drawn(world, view) {
        return;
    }
    let (Some(camera), Some(msaa), Some(offset)) = (
        world.get::<ExtractedCamera>(view),
        world.get::<Msaa>(view),
        world.get::<ViewUniformOffset>(view),
    ) else {
        return;
    };
    let Some(gpu) = world.get_resource::<AtmosphereGpu>() else {
        return;
    };
    let Some(bind_group) = gpu.bind_group.as_ref() else {
        return;
    };
    let cache = world.resource::<PipelineCache>();
    let key = AtmospherePipelineKey {
        msaa: *msaa,
        hdr: camera.hdr,
        stars: false,
        skybox: false,
    };
    let Some(stock) = gpu
        .pipelines
        .get(&key)
        .and_then(|id| cache.get_render_pipeline(*id))
    else {
        return;
    };
    pass.set_render_pipeline(stock);
    pass.set_bind_group(0, bind_group, &[offset.offset]);
    pass.draw(0..3, 0..1);
    for (stars, skybox) in [(true, false), (false, true)] {
        if skybox && !gpu.skybox_active {
            continue;
        }
        let key = AtmospherePipelineKey {
            stars,
            skybox,
            ..key
        };
        if let Some(pipeline) = gpu
            .pipelines
            .get(&key)
            .and_then(|id| cache.get_render_pipeline(*id))
        {
            pass.set_render_pipeline(pipeline);
            if stars {
                pass.draw(3..3 + gpu.star_vertex_count, 0..1);
            } else {
                pass.draw(0..3, 0..1);
            }
        }
    }
}
