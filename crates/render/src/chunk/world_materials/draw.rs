//! Queueing and drawing the world material terrain passes.

use super::*;
use bevy::ecs::system::lifetimeless::SQuery;
use bevy::render::render_resource::CachedRenderPipelineId;
use bevy::shader::Shader;
use std::collections::HashMap;

/// World material slots each section carries, by entity and mesh generation, so a section is
/// scanned once per mesh, and the sections each view draws this frame.
#[derive(Resource, Default)]
pub(in crate::chunk) struct WorldTerrainSections {
    masks: HashMap<Entity, (u64, u64)>,
    /// Per view: the sections it draws, with their slot masks, and its cube and model fallback
    /// pipelines.
    views: HashMap<Entity, ViewDraws>,
}

/// One view's tagged sections with their slot masks, and its cube and model fallback pipelines.
type ViewDraws = (Vec<(Entity, u64)>, [CachedRenderPipelineId; 2]);

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
fn ready<'w>(
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

/// Queues the opaque world material item of each view that sees a tagged section, and one
/// sorted translucent item per section and translucent material, at the section distance
/// ordinary translucent terrain sorts by.
#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn queue_world_material_chunks(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<ChunkPipeline>,
    world: Option<Res<WorldMaterialGpu>>,
    mut sections: ResMut<WorldTerrainSections>,
    (mut opaque_phases, mut transparent_phases): (
        ResMut<ViewBinnedRenderPhases<Opaque3d>>,
        ResMut<ViewSortedRenderPhases<Transparent3d>>,
    ),
    (draw_functions, transparent_functions): (
        Res<DrawFunctions<Opaque3d>>,
        Res<DrawFunctions<Transparent3d>>,
    ),
    views: Query<super::super::draw::ChunkViewQuery>,
    instances: Query<(&ChunkRenderInstance, &MainEntity)>,
    pack: render_scene::shaderpack::ShaderPackDrawnViews,
    failures: Option<Res<render_scene::world_material::WorldMaterialFailures>>,
) {
    let sections = &mut *sections;
    sections.views.clear();
    let Some(world) = world.filter(|world| world.has_materials()) else {
        sections.masks.clear();
        return;
    };
    let opaque_draw = draw_functions
        .read()
        .id::<DrawWorldMaterialTerrainCommands>();
    let translucent_draw = transparent_functions
        .read()
        .id::<DrawWorldMaterialTranslucentCommands>();
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
            let Ok((instance, _)) = instances.get(*entity) else {
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
        if let Some(phase) = transparent_phases.get_mut(&view.retained_view_entity) {
            let rangefinder = view.rangefinder3d();
            for &(entity, mask) in &draws {
                let Ok((instance, main)) = instances.get(entity) else {
                    continue;
                };
                // A section still drawing its fallback draws nothing translucent yet.
                if !ready_mask(mask, &world, &pipeline, &pipeline_cache, base) {
                    continue;
                }
                let (_, sorted) = split_classes(mask, |slot| {
                    world.variant(slot).map(|variant| variant.blend)
                });
                for slot in slots(sorted) {
                    let Some(variant) = world.variant(slot) else {
                        continue;
                    };
                    let Some(&id) = pipeline.world_ids.get(&WorldTerrainKey {
                        base,
                        world: variant,
                        geometry: Geometry::Cube,
                    }) else {
                        continue;
                    };
                    crate::transparent_phase::add(
                        phase,
                        Transparent3d {
                            sorting_info:
                                bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                            entity: (entity, *main),
                            pipeline: id,
                            draw_function: translucent_draw,
                            distance:
                                crate::chunk::transparent::model::transparent_model_phase_distance(
                                    &rangefinder,
                                    instance.key,
                                ),
                            batch_range: 0..1,
                            extra_index: PhaseItemExtraIndex::IndirectParametersIndex {
                                range: u32::from(slot)..u32::from(slot) + 1,
                                batch_set_index: None,
                            },
                            indexed: true,
                        },
                    );
                }
            }
        }
        sections
            .views
            .insert(view_entity, (draws, [fallback, model_fallback]));
    }
    if let Some(failures) = failures.as_deref() {
        crate::world_material::record_failures(
            &pipeline_cache,
            pipeline
                .world_ids
                .iter()
                .map(|(key, id)| (key.world.key, *id)),
            failures,
        );
    }
    // Despawned sections leave no stale masks behind.
    sections
        .masks
        .retain(|entity, _| instances.contains(*entity));
}

/// Draws one section's tagged cube quads and model draws with the current pipeline of each
/// geometry.
#[allow(clippy::too_many_arguments)]
fn draw_section<'w>(
    pass: &mut TrackedRenderPass<'w>,
    arena: &'w ChunkGpuArena,
    allocation: &GpuChunkAllocation,
    cube: Option<&'w RenderPipeline>,
    model: Option<&'w RenderPipeline>,
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
            transparent_model_direct_draw_command(allocation),
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
/// geometry with its pipelines, and every tagged geometry of a section with an unready material
/// through the Bedrock fallback pipelines. Translucent materials draw as sorted items instead.
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
        let ready_section = |mask: u64| ready_mask(mask, world, pipeline, cache, base);
        for (entity, _) in draws.iter().filter(|(_, mask)| !ready_section(*mask)) {
            if let Ok(allocation) = allocations.get(*entity) {
                draw_section(pass, arena, allocation, fallback[0], fallback[1]);
            }
        }
        let union = draws
            .iter()
            .filter(|(_, mask)| ready_section(*mask))
            .fold(0, |union, (_, mask)| union | mask);
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
            for (entity, _) in draws
                .iter()
                .filter(|(_, mask)| mask & (1 << slot) != 0 && ready_section(*mask))
            {
                if let Ok(allocation) = allocations.get(*entity) {
                    draw_section(pass, arena, allocation, Some(cube), Some(model));
                }
            }
        }
        RenderCommandResult::Success
    }
}

pub(in crate::chunk) type DrawWorldMaterialTranslucentCommands = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    DrawWorldMaterialTranslucent,
);

/// Draws one section's geometry of one translucent material, the slot its item carries.
pub(in crate::chunk) struct DrawWorldMaterialTranslucent;

impl RenderCommand<Transparent3d> for DrawWorldMaterialTranslucent {
    type Param = (
        SRes<ChunkGpuArena>,
        Option<SRes<WorldMaterialGpu>>,
        SRes<ChunkPipeline>,
        SRes<PipelineCache>,
    );
    type ViewQuery = (
        Read<ViewUniformOffset>,
        Read<Msaa>,
        Read<bevy::render::camera::ExtractedCamera>,
    );
    type ItemQuery = Read<GpuChunkAllocation>;

    fn render<'w>(
        item: &Transparent3d,
        (view_offset, msaa, camera): ROQueryItem<'w, '_, Self::ViewQuery>,
        allocation: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, world, pipeline, cache): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let PhaseItemExtraIndex::IndirectParametersIndex { range, .. } = item.extra_index() else {
            return RenderCommandResult::Skip;
        };
        let (Some(bind_group), Some(allocation), Some(world), Ok(slot)) = (
            &arena.bind_group,
            allocation,
            world,
            u16::try_from(range.start),
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
        let geometry = |geometry| ready(slot, world, pipeline, cache, base, geometry);
        let (Some((cube, group, _)), Some((model, _, _))) =
            (geometry(Geometry::Cube), geometry(Geometry::Model))
        else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_vertex_buffer(0, arena.vertex_offset_buffer.slice(..));
        pass.set_bind_group(WORLD_MATERIAL_GROUP as usize, group, &[]);
        draw_section(pass, arena, allocation, Some(cube), Some(model));
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
}
