//! The world material effects pass on terrain: each visible section's geometry bound to a
//! material with a bloom or an outline draws again into the view's effects targets.

use super::draw::{WorldTerrainSections, draw_section, slots};
use super::*;
use crate::world_material::scene::WorldSceneCopies;

/// Draws every visible section's geometry bound to a material with a bloom or an outline into
/// `pass`, whose colour attachments are the view's effects targets; returns whether any drew.
pub(crate) fn draw<'w>(
    world: &'w World,
    (view, offset, msaa, hdr): crate::actor_render::world_effects::EffectsView,
    pass: &mut TrackedRenderPass<'w>,
) -> bool {
    let (Some(arena), Some(sections), Some(pipeline), Some(cache), Some(materials), Some(lightmap)) = (
        world.get_resource::<ChunkGpuArena>(),
        world.get_resource::<WorldTerrainSections>(),
        world.get_resource::<ChunkPipeline>(),
        world.get_resource::<PipelineCache>(),
        world.get_resource::<WorldMaterialGpu>(),
        world.get_resource::<crate::lighting::LightmapGpu>(),
    ) else {
        return false;
    };
    let Some(bind_group) = &arena.bind_group else {
        return false;
    };
    let draws = sections.view(view);
    let scene = world
        .get_resource::<WorldSceneCopies>()
        .and_then(|scenes| scenes.group(view));
    let base = ChunkPipelineKey {
        msaa,
        hdr,
        enhanced: false,
    };
    let union = draws.iter().fold(0, |union, (_, mask, _)| union | mask);
    let mut drew = false;
    let mut bound = false;
    for slot in slots(union) {
        let Some((variant, group)) = materials
            .slot(slot)
            .filter(|(variant, _)| variant.effects_pass)
        else {
            continue;
        };
        if variant.features.scene && scene.is_none() {
            continue;
        }
        let effects = |geometry| {
            let id = *pipeline.world_ids.get(&WorldTerrainKey {
                base,
                world: variant,
                geometry,
                effects: true,
            })?;
            cache.get_render_pipeline(id)
        };
        let (Some(cube), Some(model)) = (effects(Geometry::Cube), effects(Geometry::Model)) else {
            continue;
        };
        if !bound {
            pass.set_bind_group(0, bind_group, &[offset]);
            pass.set_bind_group(1, &lightmap.bind_group, &[]);
            pass.set_vertex_buffer(0, arena.vertex_offset_buffer.slice(..));
            bound = true;
        }
        pass.set_bind_group(WORLD_MATERIAL_GROUP as usize, group, &[]);
        if let Some(scene) = scene.filter(|_| variant.features.scene) {
            pass.set_bind_group(WORLD_MATERIAL_GROUP as usize + 1, scene, &[]);
        }
        for (entity, _, _) in draws.iter().filter(|(_, mask, _)| mask & (1 << slot) != 0) {
            if let Some(allocation) = world.get::<GpuChunkAllocation>(*entity) {
                draw_section(pass, arena, allocation, Some(cube), Some(model), true);
                drew = true;
            }
        }
    }
    drew
}
