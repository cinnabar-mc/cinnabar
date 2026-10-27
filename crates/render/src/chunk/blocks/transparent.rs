//! Retained sections participate in the world's ordinary back-to-front transparency phase.

use super::*;

pub(super) struct Draw {
    placement: (Entity, DrawingHandle),
    range: Range<u32>,
}

pub(in crate::chunk) type DrawCommands = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    DrawSection,
);

/// Queues native-sized section ranges using the same view-depth metric as world terrain.
pub(super) fn queue(
    mut blocks: ResMut<Blocks>,
    frame: Res<DrawingFrame>,
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
        let start = draws.len();
        for (&(view_entity, id), placement) in placements.iter() {
            if view_entity != entity
                || !placement.active
                || placement.clip.is_some()
                || !frame.live_owners.contains(&(id.owner, id.generation))
            {
                continue;
            }
            let Some(order) = &placement.order else {
                continue;
            };
            for group in &order.groups {
                let index = draws.len() as u32;
                draws.push(Draw {
                    placement: (entity, id),
                    range: group.range.clone(),
                });
                crate::transparent_phase::add(
                    phase,
                    Transparent3d {
                        sorting_info:
                            bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                        distance: placement.matrix.transform_point3(group.center).z,
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
        // Native resources become ready after the ordinary phase sort. Preserve every
        // owner's authored distance when adding the newly prepared section ranges.
        if draws.len() != start {
            phase.sort();
        }
    }
}

pub(in crate::chunk) struct DrawSection;

impl RenderCommand<Transparent3d> for DrawSection {
    type Param = (SRes<Blocks>, SRes<DrawingFrame>);
    type ViewQuery = Entity;
    type ItemQuery = ();

    /// Draws only the queued section range from the placement's immutable sorted stream.
    fn render<'w>(
        item: &Transparent3d,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (blocks, frame): SystemParamItem<'w, '_, Self::Param>,
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
        let Some(binding) = &placement.binding else {
            return RenderCommandResult::Skip;
        };
        let arena = &placement.native.terrain.arena;
        pass.set_bind_group(0, binding, &[0]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(0..STATIC_QUAD_INDICES.len() as u32, 0, draw.range.clone());
        RenderCommandResult::Success
    }
}

#[cfg(test)]
mod tests;
