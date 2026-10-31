//! Retained sections participate in the world's ordinary back-to-front transparency phase.

use super::*;

pub(super) struct Draw {
    placement: (Entity, DrawingHandle),
    section: SubChunkKey,
    geometry: Arc<mod_render::retained::Geometry>,
}

pub(in crate::chunk) type DrawCommands = (crate::lighting::SetWorldLightmap, DrawSection);

/// Queues native-sized section ranges using the same view-depth metric as world terrain.
pub(super) fn queue(
    mut blocks: ResMut<Blocks>,
    frame: Res<DrawingFrame>,
    scene: Res<ModRenderScene>,
    panorama: Option<Res<render_scene::panorama::PanoramaScene>>,
    functions: Res<DrawFunctions<Transparent3d>>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    views: Query<(Entity, &ExtractedView, &MainEntity)>,
) {
    let Blocks {
        placements, draws, ..
    } = &mut *blocks;
    draws.clear();
    if frame.connection == 0 || panorama.is_some_and(|scene| !scene.game_visible()) {
        return;
    }
    let draw_function = functions.read().id::<DrawCommands>();
    for (entity, view, main) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        for (&(view_entity, id), placement) in placements.iter() {
            if view_entity != entity || !frame.live_owners.contains(&(id.owner, id.generation)) {
                continue;
            }
            let Some(order) = &placement.order else {
                continue;
            };
            let Some(instance) = scene
                .retained
                .iter()
                .filter(|resources| {
                    resources.connection == frame.connection
                        && resources.dimension == frame.dimension
                })
                .find_map(|resources| {
                    let instance = resources.instances.get(&id)?;
                    let geometry = resources.geometry.get(&instance.geometry)?;
                    (Arc::ptr_eq(geometry, &placement.native.geometry)
                        && instance.clip.is_none()
                        && instance.depth != Depth::Always)
                        .then_some(instance)
                })
            else {
                continue;
            };
            let Some(matrix) = resolve(instance, &frame, &view.world_from_view)
                .and_then(|resolved| prepare::matrix(view, &resolved))
            else {
                continue;
            };
            for &section in order.groups.keys() {
                let index = draws.len() as u32;
                draws.push(Draw {
                    placement: (entity, id),
                    section,
                    geometry: Arc::clone(&placement.native.geometry),
                });
                crate::transparent_phase::add(
                    phase,
                    Transparent3d {
                        sorting_info:
                            bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                        distance: matrix
                            .transform_point3(
                                crate::chunk::transparent::model::transparent_model_subchunk_center(
                                    section,
                                ),
                            )
                            .z,
                        pipeline: placement.pipelines[3],
                        entity: (entity, *main),
                        draw_function,
                        batch_range: index..index + 1,
                        extra_index: PhaseItemExtraIndex::None,
                        indexed: true,
                    },
                );
            }
        }
    }
}

pub(in crate::chunk) struct DrawSection;

impl RenderCommand<Transparent3d> for DrawSection {
    type Param = (SRes<Blocks>, SRes<DrawingFrame>, SRes<PipelineCache>);
    type ViewQuery = Entity;
    type ItemQuery = ();

    /// Draws only the queued section range from the placement's immutable sorted stream.
    fn render<'w>(
        item: &Transparent3d,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (blocks, frame, cache): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (blocks, frame) = (blocks.into_inner(), frame.into_inner());
        let Some(draw) = blocks.draws.get(item.batch_range.start as usize) else {
            return RenderCommandResult::Skip;
        };
        if draw.placement.0 != view
            || frame.connection == 0
            || !frame
                .live_owners
                .contains(&(draw.placement.1.owner, draw.placement.1.generation))
        {
            return RenderCommandResult::Skip;
        }
        let Some(placement) = blocks.placements.get(&draw.placement) else {
            return RenderCommandResult::Skip;
        };
        if !placement.active
            || placement.clip.is_some()
            || !Arc::ptr_eq(&draw.geometry, &placement.native.geometry)
        {
            return RenderCommandResult::Skip;
        }
        let (Some(binding), Some(range), Some(pipeline)) = (
            &placement.binding,
            placement
                .order
                .as_ref()
                .and_then(|order| order.groups.get(&draw.section)),
            cache
                .into_inner()
                .get_render_pipeline(placement.pipelines[3]),
        ) else {
            return RenderCommandResult::Skip;
        };
        let arena = &placement.native.terrain.arena;
        pass.set_render_pipeline(pipeline);
        pass.set_bind_group(0, binding, &[0]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(0..STATIC_QUAD_INDICES.len() as u32, 0, range.clone());
        RenderCommandResult::Success
    }
}

#[cfg(test)]
mod tests;
