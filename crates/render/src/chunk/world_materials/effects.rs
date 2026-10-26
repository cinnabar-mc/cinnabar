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

/// Each section's bound blocks by entity, as [`bound_blocks`] lists them, with the mesh
/// generation they were read from, so a section is scanned once per mesh.
#[derive(Default)]
pub(crate) struct SectionBlocks(std::collections::HashMap<Entity, (u64, BoundBlocks)>);

/// A section's bound blocks with their slots, sorted, as [`bound_blocks`] lists them.
type BoundBlocks = Box<[(u16, [i32; 3])]>;

impl SectionBlocks {
    /// Reads `instance`'s bound blocks unless they were read from its current mesh generation.
    fn refresh(&mut self, entity: Entity, instance: &ChunkRenderInstance) {
        match self.0.get(&entity) {
            Some((generation, _)) if *generation == instance.generation => {}
            _ => {
                self.0
                    .insert(entity, (instance.generation, bound_blocks(instance)));
            }
        }
    }

    /// The bound blocks last read for `entity`; none before its first [`Self::refresh`].
    fn blocks(&self, entity: Entity) -> &[(u16, [i32; 3])] {
        self.0.get(&entity).map_or(&[], |(_, blocks)| blocks)
    }
}

/// Gathers, for each view, the blocks of its visible sections bound to a material with
/// particles, at their centres, for the particle pass.
pub(crate) fn gather_particles(
    sections: Option<Res<WorldTerrainSections>>,
    instances: Query<&ChunkRenderInstance>,
    materials: Option<Res<WorldMaterialGpu>>,
    mut targets: ResMut<crate::world_material::particles::ParticleTargets>,
    mut cached: Local<SectionBlocks>,
) {
    targets.blocks.clear();
    let (Some(sections), Some(materials)) = (sections, materials) else {
        cached.0.clear();
        return;
    };
    if !materials.has_particles() {
        cached.0.clear();
        return;
    }
    // Despawned sections leave no stale blocks behind.
    cached.0.retain(|entity, _| instances.contains(*entity));
    let particle_slots = (0..64u16)
        .filter(|slot| materials.particles(*slot).is_some())
        .fold(0_u64, |mask, slot| mask | 1 << slot);
    for (view, draws) in sections.all_views() {
        let emitting = || {
            draws
                .iter()
                .filter(|(_, mask, _)| mask & particle_slots != 0)
                .filter_map(|(entity, mask, _)| {
                    Some((*entity, *mask, instances.get(*entity).ok()?))
                })
        };
        for (entity, _, instance) in emitting() {
            cached.refresh(entity, instance);
        }
        let lists = emitting().map(|(entity, mask, _)| (cached.blocks(entity), mask));
        let gathered = merge(lists, particle_slots);
        if !gathered.is_empty() {
            targets.blocks.insert(view, gathered);
        }
    }
}

/// The blocks of `lists` (each a section's sorted bound blocks with the slots it carries)
/// whose slot is in `particle_slots`, by slot, at their centres: each slot's run of every
/// section in turn, in section order, so no frame sorts them. Sections hold no block in common,
/// so each block appears once.
fn merge<'a>(
    lists: impl IntoIterator<Item = (&'a [(u16, [i32; 3])], u64)>,
    particle_slots: u64,
) -> std::collections::BTreeMap<u16, Vec<[f32; 4]>> {
    let mut gathered = std::collections::BTreeMap::<u16, Vec<[f32; 4]>>::new();
    for (blocks, mask) in lists {
        let mut wanted = mask & particle_slots;
        while wanted != 0 {
            let slot = wanted.trailing_zeros() as u16;
            wanted &= wanted - 1;
            let start = blocks.partition_point(|(held, _)| *held < slot);
            let end = blocks.partition_point(|(held, _)| *held <= slot);
            if start == end {
                continue;
            }
            gathered.entry(slot).or_default().extend(
                blocks[start..end]
                    .iter()
                    .map(|(_, [x, y, z])| [*x as f32 + 0.5, *y as f32 + 0.5, *z as f32 + 0.5, 1.0]),
            );
        }
    }
    gathered
}

/// Every block of `instance` whose geometry carries a world material, with its slot, in world
/// block coordinates, sorted and each once. A tagged cube quad names each block it covers; a
/// tagged model draw names its model ref's block, so blocks drawn only as models count too.
fn bound_blocks(instance: &ChunkRenderInstance) -> Box<[(u16, [i32; 3])]> {
    let mut blocks = Vec::new();
    let mut add = |tag: u8, local: [u8; 3]| {
        if tag != 0 {
            let block = std::array::from_fn(|axis| instance.origin[axis] + i32::from(local[axis]));
            blocks.push((u16::from(tag - 1), block));
        }
    };
    for quad in instance.cube_quads.iter() {
        if quad.world_material() != 0 {
            for local in quad.blocks() {
                add(quad.world_material(), local);
            }
        }
    }
    let draws = instance
        .model_draw_refs
        .iter()
        .chain(instance.transparent_model_draw_refs.iter());
    for draw in draws {
        if let Some(model) = instance.model_refs.get(draw.model_ref_index() as usize) {
            add(draw.world_material(), model.local());
        }
    }
    blocks.sort_unstable();
    blocks.dedup();
    blocks.into_boxed_slice()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A section at `origin` holding only a model block at local `[3, 4, 5]`, its draw tagged
    /// with `tag`.
    fn model_section(origin: [i32; 3], tag: u8) -> ChunkRenderInstance {
        let transform = 3 | (4 << 4) | (5 << 8);
        ChunkRenderInstance {
            key: SubChunkKey::new(0, 0, 0, 0),
            cube_quads: Arc::from([]),
            cube_lighting: Arc::from([]),
            cube_layout: Default::default(),
            model_refs: Arc::from([meshing::PackedModelRef::new(transform, 0, 0, 1)]),
            model_lighting: Arc::from([]),
            model_draw_refs: Arc::from([
                meshing::PackedModelDrawRef::new(0, 0).with_world_material(tag)
            ]),
            transparent_model_draw_refs: Arc::from([]),
            liquid_quads: Arc::from([]),
            liquid_lighting: Arc::from([]),
            has_depth_liquid: false,
            has_transparent_liquid: false,
            depth_liquid_start: None,
            biome: meshing::PackedBiomeRecord::fallback(),
            tint_identity: ChunkBiomeTintIdentity::default(),
            generation: 1,
            priority: render_scene::ChunkUploadPriority::new(0.0),
            token: None,
            publication_permit: None,
            origin,
            world_materials: tag != 0,
        }
    }

    #[test]
    fn blocks_drawn_only_as_models_are_particle_targets() {
        let blocks = bound_blocks(&model_section([16, 32, 48], 3));
        let expected = std::collections::BTreeMap::from([(2_u16, vec![[19.5, 36.5, 53.5, 1.0]])]);
        assert_eq!(
            merge([(&*blocks, u64::MAX)], 0b100),
            expected,
            "the model block emits from its own cell"
        );
        assert!(
            merge([(&*blocks, u64::MAX)], 0b011).is_empty(),
            "slots without particles gather nothing"
        );
    }

    #[test]
    fn merged_bound_faces_emit_from_every_block_they_cover() {
        let mut section = model_section([0, 16, 0], 0);
        // One top face over three blocks along x, tagged with slot 0, and an untagged one.
        section.cube_quads = Arc::from([
            meshing::PackedQuad::new([1, 2, 3], meshing::Face::PositiveY, 3, 1, 0)
                .with_world_material(1),
            meshing::PackedQuad::new([0, 0, 0], meshing::Face::PositiveY, 2, 2, 0),
        ]);
        assert_eq!(
            &*bound_blocks(&section),
            [(0, [1, 18, 3]), (0, [2, 18, 3]), (0, [3, 18, 3])],
            "untagged geometry and untagged model draws add nothing"
        );
    }

    #[test]
    fn cached_section_blocks_are_read_again_only_for_a_new_mesh() {
        let entity = Entity::from_raw_u32(7).unwrap();
        let mut cached = SectionBlocks::default();
        let mut section = model_section([0; 3], 1);
        cached.refresh(entity, &section);
        let first = cached.blocks(entity).to_vec();
        assert_eq!(first, [(0, [3, 4, 5])]);
        // A frame with the same mesh generation reuses what was read.
        section.model_draw_refs =
            Arc::from([meshing::PackedModelDrawRef::new(0, 0).with_world_material(2)]);
        cached.refresh(entity, &section);
        assert_eq!(cached.blocks(entity), first.as_slice());
        // A new mesh generation reads the section again.
        section.generation += 1;
        cached.refresh(entity, &section);
        assert_eq!(cached.blocks(entity), [(1, [3, 4, 5])].as_slice());
        // Sections merge into one list per slot, section by section, and only the slots a
        // section carries are looked up.
        let other = bound_blocks(&model_section([16, 0, 0], 1));
        assert_eq!(
            merge([(&*other, 0b1), (first.as_slice(), 0b1)], 0b1),
            std::collections::BTreeMap::from([(
                0_u16,
                vec![[19.5, 4.5, 5.5, 1.0], [3.5, 4.5, 5.5, 1.0]]
            )])
        );
        assert!(merge([(&*other, 0b10)], 0b11).is_empty());
    }
}
