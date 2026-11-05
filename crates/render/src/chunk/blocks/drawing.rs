//! Encodes native terrain against world depth or a clip's independent depth.
use super::*;
use bevy::render::renderer::{RenderContext, ViewQuery};
use bevy::render::view::ViewDepthStencilTexture;

/// Draws every ready block placement for this view and clip through native terrain programs.
pub(crate) fn encode<'w>(
    world: &'w World,
    entity: Entity,
    clip: Option<DrawingHandle>,
    transparent: bool,
    always: Option<bool>,
    pass: &mut TrackedRenderPass<'w>,
) {
    if !contains(world, entity, clip, always) {
        return;
    }
    let (Some(blocks), Some(cache), Some(lightmap)) = (
        world.get_resource::<Blocks>(),
        world.get_resource::<PipelineCache>(),
        world.get_resource::<crate::lighting::LightmapGpu>(),
    ) else {
        return;
    };
    let Some(frame) = world.get_resource::<DrawingFrame>() else {
        return;
    };
    for (&(view, id), placement) in &blocks.placements {
        if frame.connection == 0
            || !frame.live_owners.contains(&(id.owner, id.generation))
            || view != entity
            || !placement.active
            || placement.clip != clip
            || always.is_some_and(|always| always != placement.always)
        {
            continue;
        }
        let arena = &placement.native.terrain.arena;
        let (Some(binding), Some(opaque)) = (&placement.binding, &placement.opaque_binding) else {
            continue;
        };
        pass.set_bind_group(1, &lightmap.bind_group, &[]);
        if transparent {
            let (Some(order), Some(pipeline)) = (
                &placement.order,
                cache.get_render_pipeline(placement.pipelines[3]),
            ) else {
                continue;
            };
            if order.count == 0 {
                continue;
            }
            pass.set_render_pipeline(pipeline);
            pass.set_bind_group(0, binding, &[0]);
            pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
            pass.draw_indexed(0..STATIC_QUAD_INDICES.len() as u32, 0, 0..order.count);
            continue;
        }
        pass.set_bind_group(0, opaque, &[0]);
        pass.set_vertex_buffer(0, arena.vertex_offset_buffer.slice(..));
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        let camera = placement
            .matrix
            .inverse()
            .w_axis
            .truncate()
            .as_dvec3()
            .to_array();
        for allocation in arena.allocations.values() {
            let allocation = &allocation.gpu;
            if let (Some(pipeline), Some(commands)) = (
                cache.get_render_pipeline(placement.pipelines[1]),
                solid_indirect_commands(allocation, Some(camera)),
            ) {
                pass.set_render_pipeline(pipeline);
                for args in commands {
                    issue(pass, args);
                }
            }
            for (variant, args) in [
                (0, cutout_indirect_command(allocation)),
                (2, model_direct_draw_command(allocation)),
                (4, depth_liquid_direct_draw_command(allocation)),
            ] {
                if let (Some(pipeline), Some(args)) = (
                    cache.get_render_pipeline(placement.pipelines[variant]),
                    args,
                ) {
                    pass.set_render_pipeline(pipeline);
                    issue(pass, args);
                }
            }
        }
    }
}

/// Emits the native uploader's validated indexed range.
fn issue(pass: &mut TrackedRenderPass<'_>, args: DrawIndexedIndirectArgs) {
    pass.draw_indexed(
        args.first_index..args.first_index + args.index_count,
        args.base_vertex,
        args.first_instance..args.first_instance + args.instance_count,
    );
}

/// Whether this view has active block contents using a particular clip.
pub(crate) fn contains(
    world: &World,
    entity: Entity,
    clip: Option<DrawingHandle>,
    always: Option<bool>,
) -> bool {
    if world
        .get_resource::<render_scene::panorama::PanoramaScene>()
        .is_some_and(|scene| !scene.game_visible())
    {
        return false;
    }
    world.get_resource::<Blocks>().is_some_and(|blocks| {
        blocks.placements.iter().any(|(&(view, _), placement)| {
            view == entity
                && placement.active
                && placement.clip == clip
                && always.is_none_or(|always| always == placement.always)
        })
    })
}

/// Draws unclipped block grids in ordinary terrain colour spaces and against world depth.
#[allow(
    clippy::type_complexity,
    reason = "the view pass borrows Bevy's target and viewport components"
)]
pub(super) fn world_draw<const OVERLAY: bool>(
    world: &World,
    query: ViewQuery<(
        &ViewTarget,
        &ViewDepthStencilTexture,
        &ExtractedCamera,
        Option<&bevy::camera::MainPassResolutionOverride>,
    )>,
    mut context: RenderContext,
) -> bevy::ecs::error::Result {
    let entity = query.entity();
    let (target, depth, camera, resolution) = query.into_inner();
    if !contains(world, entity, None, Some(OVERLAY)) {
        return Ok(());
    }
    let extent = [depth.texture().width(), depth.texture().height()];
    let Some(viewport) =
        crate::render_bounds::camera_viewport(camera.viewport.as_ref(), resolution, extent)
    else {
        return Ok(());
    };
    for transparent in [false, true].into_iter().take(if OVERLAY { 2 } else { 1 }) {
        let Some(color) = crate::scene_target::retained_color_attachment(
            world,
            entity,
            target,
            transparent && !camera.hdr,
        ) else {
            return Ok(());
        };
        let colors = [Some(color)];
        let mut pass = context.begin_tracked_render_pass(
            bevy::render::render_resource::RenderPassDescriptor {
                label: Some("retained terrain"),
                color_attachments: &colors,
                depth_stencil_attachment: Some(
                    depth.get_attachment(bevy::render::render_resource::StoreOp::Store),
                ),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            },
        );
        pass.set_camera_viewport(&viewport);
        pass.set_scissor_rect(
            viewport.physical_position.x,
            viewport.physical_position.y,
            viewport.physical_size.x,
            viewport.physical_size.y,
        );
        encode(world, entity, None, transparent, Some(OVERLAY), &mut pass);
    }
    Ok(())
}
