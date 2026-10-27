//! Queueing and drawing the world material terrain passes.

use super::*;
use bevy::ecs::system::lifetimeless::SQuery;
use bevy::render::render_resource::CachedRenderPipelineId;
use bevy::shader::Shader;
use std::collections::{HashMap, HashSet};

/// World material slots each section carries, by entity and mesh generation, so a section is
/// scanned once per mesh, and the sections each view draws this frame.
#[derive(Resource, Default)]
pub(in crate::chunk) struct WorldTerrainSections {
    masks: HashMap<Entity, (u64, u64)>,
    /// Per view: the sections it draws, with their slot masks, and its cube and model fallback
    /// pipelines.
    views: HashMap<Entity, ViewDraws>,
    /// Per view: its sections with ready translucent materials and their slot masks, which
    /// the transparent queue merges into those sections' ordinary translucent order this frame.
    translucent: HashMap<Entity, HashMap<Entity, u64>>,
    /// Per view: its sections still drawing their Bedrock fallback, whose tagged translucent
    /// model draws the transparent queue draws as Bedrock in their ordinary translucent order.
    bedrock: HashMap<Entity, HashSet<Entity>>,
    /// Whether a section's mesh may still carry a tag: set while any material is published,
    /// cleared once none is and no section's mesh carries a tag.
    tagged: bool,
}

impl WorldTerrainSections {
    /// Takes `view`'s sections with ready translucent materials queued this frame.
    pub(in crate::chunk) fn take_translucent(&mut self, view: Entity) -> HashMap<Entity, u64> {
        self.translucent.remove(&view).unwrap_or_default()
    }

    /// Takes `view`'s sections queued this frame with their Bedrock fallback.
    pub(in crate::chunk) fn take_bedrock(&mut self, view: Entity) -> HashSet<Entity> {
        self.bedrock.remove(&view).unwrap_or_default()
    }

    /// Whether this frame queues the world material terrain passes: while any material is
    /// published, and after the last is revoked until no section's mesh carries a tag, so a
    /// mesh tagged before revocation draws its Bedrock fallback until its remesh replaces it
    /// rather than vanishing. Without materials it rescans every section in `all`, each with
    /// its mesh generation, not only the visible ones; `mask_of` reads a section's slot mask.
    fn queueing(
        &mut self,
        has_materials: bool,
        all: impl Iterator<Item = (Entity, u64)>,
        mask_of: impl Fn(Entity) -> u64,
    ) -> bool {
        if has_materials {
            self.tagged = true;
            return true;
        }
        if self.tagged {
            let mut masks = HashMap::new();
            for (entity, generation) in all {
                let mask = match self.masks.get(&entity) {
                    Some((cached, mask)) if *cached == generation => *mask,
                    _ => mask_of(entity),
                };
                masks.insert(entity, (generation, mask));
            }
            self.masks = masks;
            self.tagged = self.masks.values().any(|(_, mask)| *mask != 0);
        }
        if !self.tagged {
            self.masks.clear();
        }
        self.tagged
    }
}

/// One view's tagged sections with their slot masks and whether all their materials are ready,
/// and its cube and model fallback pipelines.
type ViewDraws = (Vec<(Entity, u64, bool)>, [CachedRenderPipelineId; 2]);

/// One view's tagged sections as queued this frame.
#[derive(Debug, Default, PartialEq)]
struct ViewPlan {
    /// Each section with its slot mask and whether every material it carries is ready.
    draws: Vec<(Entity, u64, bool)>,
    /// The ready sections with translucent materials, and those materials' slots.
    translucent: HashMap<Entity, u64>,
    /// The sections drawing their Bedrock fallback.
    bedrock: HashSet<Entity>,
}

/// Decides once per frame which of `draws` draw their world materials and which their Bedrock
/// fallback, so the opaque and translucent passes agree. A ready section's translucent classes
/// merge into its translucent order; a falling-back section draws its cube and opaque model
/// geometry through the opaque fallback pipelines and its translucent model draws in the
/// ordinary translucent pass, as Bedrock draws them.
fn plan_view(
    draws: Vec<(Entity, u64)>,
    ready: impl Fn(u64) -> bool,
    blend_of: impl Fn(u16) -> Option<Blend>,
) -> ViewPlan {
    let mut plan = ViewPlan::default();
    for (entity, mask) in draws {
        let ready = ready(mask);
        if !ready {
            plan.bedrock.insert(entity);
        } else {
            let (_, sorted) = split_classes(mask, &blend_of);
            if sorted != 0 {
                plan.translucent.insert(entity, sorted);
            }
        }
        plan.draws.push((entity, mask, ready));
    }
    plan
}

/// Every slot in `mask`.
fn slots(mask: u64) -> impl Iterator<Item = u16> {
    (0..64u16).filter(move |slot| mask & (1 << slot) != 0)
}

/// Splits a section's slot mask into its opaque-phase and translucent-phase materials by
/// their blend class; a slot without a material belongs to neither.
pub(in crate::chunk) fn split_classes(
    mask: u64,
    blend_of: impl Fn(u16) -> Option<Blend>,
) -> (u64, u64) {
    slots(mask).fold((0, 0), |(opaque, sorted), slot| match blend_of(slot) {
        Some(blend) if translucent(blend) => (opaque, sorted | (1 << slot)),
        Some(_) => (opaque | (1 << slot), sorted),
        None => (opaque, sorted),
    })
}

/// Whether `world` has a compiled pipeline of both geometries for every material in `mask`.
fn ready_mask(
    mask: u64,
    world: &WorldMaterialGpu,
    pipeline: &ChunkPipeline,
    cache: &PipelineCache,
    base: ChunkPipelineKey,
) -> bool {
    slots(mask).all(|slot| {
        [Geometry::Cube, Geometry::Model]
            .into_iter()
            .all(|geometry| ready(slot, world, pipeline, cache, base, geometry).is_some())
    })
}

/// The compiled pipeline and bind group of `slot`'s material for `geometry`, once both exist.
pub(in crate::chunk) fn ready<'w>(
    slot: u16,
    world: &'w WorldMaterialGpu,
    pipeline: &ChunkPipeline,
    cache: &'w PipelineCache,
    base: ChunkPipelineKey,
    geometry: Geometry,
) -> Option<(&'w RenderPipeline, &'w BindGroup, WorldVariant)> {
    let (variant, group) = world.slot(slot)?;
    let id = *pipeline.world_ids.get(&WorldTerrainKey {
        base,
        world: variant,
        geometry,
    })?;
    Some((cache.get_render_pipeline(id)?, group, variant))
}

/// Queues the opaque world material item of each view that sees a tagged section, and records
/// the view's sections whose translucent materials are ready, which the transparent queue
/// draws merged into their ordinary translucent order.
#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn queue_world_material_chunks(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<ChunkPipeline>,
    world: Option<Res<WorldMaterialGpu>>,
    mut sections: ResMut<WorldTerrainSections>,
    mut opaque_phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    draw_functions: Res<DrawFunctions<Opaque3d>>,
    views: Query<super::super::draw::ChunkViewQuery>,
    instances: Query<(Entity, &ChunkRenderInstance)>,
    pack: render_scene::shaderpack::ShaderPackDrawnViews,
    failures: Option<Res<render_scene::world_material::WorldMaterialFailures>>,
) {
    let sections = &mut *sections;
    sections.views.clear();
    sections.translucent.clear();
    sections.bedrock.clear();
    let Some(world) = world else {
        sections.masks.clear();
        return;
    };
    let all = instances
        .iter()
        .map(|(entity, instance)| (entity, instance.generation));
    let mask_of = |entity| {
        instances
            .get(entity)
            .map_or(0, |(_, instance)| slot_mask(instance))
    };
    let pipeline = &mut *pipeline;
    crate::world_material::prune_pipelines(
        &mut pipeline.world_ids,
        &mut pipeline.world_pruned,
        &world,
        |key| key.world.program,
    );
    if !sections.queueing(world.has_materials(), all, mask_of) {
        return;
    }
    let opaque_draw = draw_functions
        .read()
        .id::<DrawWorldMaterialTerrainCommands>();
    for (view_entity, view_main_entity, view, visible_entities, msaa, enhanced, camera) in &views {
        if pack.replaces(view_entity, |drawn| drawn.terrain) || enhanced.is_some() {
            continue;
        }
        let mut draws = Vec::new();
        for (entity, _) in visible_entities
            .get::<ChunkRenderInstance>()
            .into_iter()
            .flat_map(|class| class.iter_visible())
        {
            let Ok((_, instance)) = instances.get(*entity) else {
                continue;
            };
            let mask = match sections.masks.get(entity) {
                Some((generation, mask)) if *generation == instance.generation => *mask,
                _ => {
                    let mask = slot_mask(instance);
                    sections.masks.insert(*entity, (instance.generation, mask));
                    mask
                }
            };
            if mask != 0 {
                draws.push((*entity, mask));
            }
        }
        if draws.is_empty() {
            continue;
        }
        let base = ChunkPipelineKey {
            msaa: *msaa,
            hdr: camera.hdr,
            enhanced: false,
        };
        let union = draws.iter().fold(0, |union, (_, mask)| union | mask);
        for slot in slots(union) {
            let Some(variant) = world.variant(slot) else {
                continue;
            };
            for geometry in [Geometry::Cube, Geometry::Model] {
                let key = WorldTerrainKey {
                    base,
                    world: variant,
                    geometry,
                };
                let variants = match geometry {
                    Geometry::Cube => &mut pipeline.world_variants,
                    Geometry::Model => &mut pipeline.world_model_variants,
                };
                if let Ok(id) = variants.specialize(&pipeline_cache, key) {
                    pipeline.world_ids.insert(key, id);
                }
            }
        }
        let (Ok(fallback), Ok(model_fallback)) = (
            pipeline.world_fallback.specialize(&pipeline_cache, base),
            pipeline
                .world_model_fallback
                .specialize(&pipeline_cache, base),
        ) else {
            continue;
        };
        if let Some(phase) = opaque_phases.get_mut(&view.retained_view_entity) {
            phase.add(
                Opaque3dBatchSetKey {
                    draw_function: opaque_draw,
                    pipeline: fallback,
                    material_bind_group_index: None,
                    lightmap_slab: None,
                    slabs: default(),
                },
                Opaque3dBinKey {
                    asset_id: AssetId::<Shader>::default().untyped(),
                },
                (view_entity, *view_main_entity),
                InputUniformIndex::default(),
                BinnedRenderPhaseType::NonMesh,
            );
        }
        let plan = plan_view(
            draws,
            |mask| ready_mask(mask, &world, pipeline, &pipeline_cache, base),
            |slot| world.variant(slot).map(|variant| variant.blend),
        );
        if !plan.translucent.is_empty() {
            sections.translucent.insert(view_entity, plan.translucent);
        }
        if !plan.bedrock.is_empty() {
            sections.bedrock.insert(view_entity, plan.bedrock);
        }
        sections
            .views
            .insert(view_entity, (plan.draws, [fallback, model_fallback]));
    }
    if let Some(failures) = failures.as_deref() {
        crate::world_material::record_failures(
            &pipeline_cache,
            pipeline
                .world_ids
                .iter()
                .map(|(key, id)| (key.world.program, *id)),
            &world,
            failures,
        );
    }
    // Despawned sections leave no stale masks behind.
    sections
        .masks
        .retain(|entity, _| instances.contains(*entity));
}

/// Draws one section's tagged cube quads and model draws with the current pipeline of each
/// geometry; its translucent model draws only when `translucent_models`.
#[allow(clippy::too_many_arguments)]
fn draw_section<'w>(
    pass: &mut TrackedRenderPass<'w>,
    arena: &'w ChunkGpuArena,
    allocation: &GpuChunkAllocation,
    cube: Option<&'w RenderPipeline>,
    model: Option<&'w RenderPipeline>,
    translucent_models: bool,
) {
    if let Some(cube) = cube
        && let Some((quads, _, base_vertex)) = cube_draw_base(allocation)
    {
        pass.set_render_pipeline(cube);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(0..STATIC_QUAD_INDICES.len() as u32, base_vertex, quads);
    }
    if let Some(model) = model {
        let commands = [
            model_direct_draw_command(allocation),
            transparent_model_direct_draw_command(allocation).filter(|_| translucent_models),
        ];
        let mut bound = false;
        for command in commands.into_iter().flatten() {
            if !bound {
                pass.set_render_pipeline(model);
                pass.set_index_buffer(arena.model_index_buffer.slice(..), IndexFormat::Uint32);
                bound = true;
            }
            pass.draw_indexed(
                command.first_index..command.first_index + command.index_count,
                command.base_vertex,
                command.first_instance..command.first_instance + command.instance_count,
            );
        }
    }
}

pub(in crate::chunk) type DrawWorldMaterialTerrainCommands = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    DrawWorldMaterialTerrain,
);

/// Draws a view's tagged sections in the opaque phase: each ready opaque or cutout material's
/// geometry with its pipelines, and the tagged cube and opaque model geometry of a section with
/// an unready material through the Bedrock fallback pipelines. Translucent materials, and the
/// translucent model draws of a falling-back section, draw as sorted items instead.
pub(in crate::chunk) struct DrawWorldMaterialTerrain;

impl<P: PhaseItem> RenderCommand<P> for DrawWorldMaterialTerrain {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<WorldTerrainSections>,
        Option<SRes<WorldMaterialGpu>>,
        SRes<ChunkPipeline>,
        SRes<PipelineCache>,
        SQuery<Read<GpuChunkAllocation>>,
    );
    type ViewQuery = (
        Entity,
        Read<ViewUniformOffset>,
        Read<Msaa>,
        Read<bevy::render::camera::ExtractedCamera>,
    );
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        (view_entity, view_offset, msaa, camera): ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, sections, world, pipeline, cache, allocations): SystemParamItem<
            'w,
            '_,
            Self::Param,
        >,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let (Some(bind_group), Some((draws, fallback)), Some(world)) = (
            &arena.bind_group,
            sections.into_inner().views.get(&view_entity),
            world,
        ) else {
            return RenderCommandResult::Skip;
        };
        let world = world.into_inner();
        let pipeline = pipeline.into_inner();
        let cache = cache.into_inner();
        let base = ChunkPipelineKey {
            msaa: *msaa,
            hdr: camera.hdr,
            enhanced: false,
        };
        let fallback = fallback.map(|id| cache.get_render_pipeline(id));
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_vertex_buffer(0, arena.vertex_offset_buffer.slice(..));
        // Readiness was decided when queued, so this pass agrees with the translucent one.
        for (entity, _, _) in draws.iter().filter(|(_, _, ready)| !ready) {
            if let Ok(allocation) = allocations.get(*entity) {
                draw_section(pass, arena, allocation, fallback[0], fallback[1], false);
            }
        }
        let union = draws
            .iter()
            .filter(|(_, _, ready)| *ready)
            .fold(0, |union, (_, mask, _)| union | mask);
        // Translucent materials draw as sorted items, never here.
        let (opaque, _) = split_classes(union, |slot| {
            world.variant(slot).map(|variant| variant.blend)
        });
        for slot in slots(opaque) {
            let geometry = |geometry| ready(slot, world, pipeline, cache, base, geometry);
            let (Some((cube, group, _)), Some((model, _, _))) =
                (geometry(Geometry::Cube), geometry(Geometry::Model))
            else {
                continue;
            };
            pass.set_bind_group(WORLD_MATERIAL_GROUP as usize, group, &[]);
            for (entity, _, _) in draws
                .iter()
                .filter(|(_, mask, ready)| *ready && mask & (1 << slot) != 0)
            {
                if let Ok(allocation) = allocations.get(*entity) {
                    draw_section(pass, arena, allocation, Some(cube), Some(model), true);
                }
            }
        }
        RenderCommandResult::Success
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translucent_classes_leave_the_opaque_phase_for_sorted_items() {
        let blend_of = |slot: u16| match slot {
            0 => Some(Blend::Opaque),
            1 => Some(Blend::Cutout),
            2 => Some(Blend::Premultiplied),
            3 => Some(Blend::Additive),
            _ => None,
        };
        let (opaque, sorted) = split_classes(0b1_1111, blend_of);
        assert_eq!(opaque, 0b0011);
        assert_eq!(sorted, 0b1100, "premultiplied and additive draw sorted");
        assert_eq!(split_classes(0, blend_of), (0, 0));
    }

    #[test]
    fn a_section_with_an_unready_material_draws_its_translucent_models_as_bedrock() {
        let blend_of = |slot: u16| match slot {
            0 => Some(Blend::Opaque),
            1 => Some(Blend::Premultiplied),
            _ => None,
        };
        let section = |index| Entity::from_raw_u32(index).unwrap();
        // Section 1's materials are ready; section 2's translucent material is not.
        let plan = plan_view(
            vec![(section(1), 0b11), (section(2), 0b10)],
            |mask| mask == 0b11,
            blend_of,
        );
        assert_eq!(
            plan.draws,
            [(section(1), 0b11, true), (section(2), 0b10, false)]
        );
        assert_eq!(plan.translucent, HashMap::from([(section(1), 0b10)]));
        assert_eq!(
            plan.bedrock,
            HashSet::from([section(2)]),
            "the translucent pass draws the unready sections' translucent models as Bedrock"
        );
    }

    #[test]
    fn revoked_materials_keep_queueing_the_fallback_until_no_mesh_carries_a_tag() {
        let section = |index| Entity::from_raw_u32(index).unwrap();
        let mut sections = WorldTerrainSections::default();
        let none = std::iter::empty();
        assert!(
            !sections.queueing(false, none, |_| 0),
            "a session without world materials queues nothing"
        );
        assert!(sections.queueing(true, std::iter::empty(), |_| 0));
        // Revoked: section 1's mesh still carries its tag, section 2's is already untagged.
        let stale = |entity| if entity == section(1) { 0b1 } else { 0 };
        let all = || [(section(1), 7), (section(2), 3)].into_iter();
        assert!(
            sections.queueing(false, all(), stale),
            "a stale tagged mesh draws its Bedrock fallback instead of vanishing"
        );
        let plan = plan_view(vec![(section(1), 0b1)], |_| false, |_| None);
        assert_eq!(plan.bedrock, HashSet::from([section(1)]));
        // Its remesh lands untagged, so the passes stop queueing.
        let remeshed = || [(section(1), 8), (section(2), 3)].into_iter();
        assert!(!sections.queueing(false, remeshed(), |_| 0));
        assert!(
            !sections.queueing(false, remeshed(), stale),
            "and stay stopped"
        );
    }
}
