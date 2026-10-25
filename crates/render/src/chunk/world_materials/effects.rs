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
    let union = draws.iter().fold(0, |union, (_, mask)| union | mask);
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
        for (entity, _) in draws.iter().filter(|(_, mask)| mask & (1 << slot) != 0) {
            if let Some(allocation) = world.get::<GpuChunkAllocation>(*entity) {
                draw_section(pass, arena, allocation, Some(cube), Some(model));
                drew = true;
            }
        }
    }
    drew
}

/// Gathers, for each view, the blocks of its visible sections bound to a material with
/// particles, at their centres, for the particle pass. A bound block's faces stay one block
/// wide, so each tagged cube quad names its own block.
pub(crate) fn gather_particles(
    sections: Option<Res<WorldTerrainSections>>,
    instances: Query<&ChunkRenderInstance>,
    materials: Option<Res<WorldMaterialGpu>>,
    mut targets: ResMut<crate::world_material::particles::ParticleTargets>,
) {
    targets.blocks.clear();
    let (Some(sections), Some(materials)) = (sections, materials) else {
        return;
    };
    if !materials.has_particles() {
        return;
    }
    let particle_slots = (0..64u16)
        .filter(|slot| materials.particles(*slot).is_some())
        .fold(0_u64, |mask, slot| mask | 1 << slot);
    for (view, draws) in sections.all_views() {
        let mut blocks: std::collections::BTreeMap<u16, std::collections::BTreeSet<[i32; 3]>> =
            Default::default();
        for (entity, mask) in draws {
            if mask & particle_slots == 0 {
                continue;
            }
            let Ok(instance) = instances.get(*entity) else {
                continue;
            };
            for quad in instance.cube_quads.iter() {
                let tag = quad.world_material();
                if tag == 0 || particle_slots & (1 << (tag - 1)) == 0 {
                    continue;
                }
                let local = quad.origin().map(i32::from);
                let block = std::array::from_fn(|axis| instance.origin[axis] + local[axis]);
                blocks.entry(u16::from(tag - 1)).or_default().insert(block);
            }
        }
        if blocks.is_empty() {
            continue;
        }
        let gathered = blocks
            .into_iter()
            .map(|(slot, cells)| {
                let origins = cells
                    .into_iter()
                    .map(|[x, y, z]| [x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5, 1.0])
                    .collect();
                (slot, origins)
            })
            .collect();
        targets.blocks.insert(view, gathered);
    }
}
