//! Programmable world materials on bound blocks. Meshing tags a bound block's cube quads with
//! its material slot; ordinary terrain pipelines skip tagged quads, and this pass draws each
//! visible section's tagged quads once per material with that material's pipeline. A section
//! whose materials are not all ready draws its tagged quads with the Bedrock fallback pipeline
//! instead, so a bound block never disappears while its material compiles.

use crate::chunk::*;
use crate::world_material::{
    ShaderFamily, TERRAIN_SHADERS, WorldMaterialGpu, WorldVariant, layout,
};
use bevy::ecs::system::lifetimeless::SQuery;
use bevy::render::render_resource::{BlendComponent, BlendFactor, BlendOperation};
use bevy::shader::Shader;
use mod_render::world_material::publication::{Blend, Depth};
use render_scene::ChunkRenderInstance;
use std::collections::HashMap;

/// The bind group a world material terrain pipeline reads its frame, textures and sampler from.
pub(crate) const WORLD_MATERIAL_GROUP: u32 = 2;
/// The terrain shader every world material's module extends.
const CHUNK_SOURCE: &str = include_str!("../chunk.wesl");

/// How terrain modules compose: the production cube shader with the library appended.
pub(crate) fn family() -> ShaderFamily {
    ShaderFamily {
        namespace: TERRAIN_SHADERS,
        group: WORLD_MATERIAL_GROUP,
        build: |library, path| {
            crate::shader_safety::from_wesl(
                crate::material_shader::source(&format!("{CHUNK_SOURCE}\n{library}")),
                path,
            )
        },
    }
}

/// A view's cutout terrain key drawn with a world material.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::chunk) struct WorldTerrainKey {
    pub(in crate::chunk) base: ChunkPipelineKey,
    pub(in crate::chunk) world: WorldVariant,
}

impl SpecializerKey for WorldTerrainKey {
    const IS_CANONICAL: bool = true;
    type Canonical = Self;
}

/// Specializes the two-sided cube pipeline with a material's module and host-owned raster: the
/// class selects blending and depth writes, and `Always` passes regardless of stored depth.
pub(in crate::chunk) struct WorldTerrainSpecializer;

impl Specializer<RenderPipeline> for WorldTerrainSpecializer {
    type Key = WorldTerrainKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        let shader = key.world.shader(TERRAIN_SHADERS);
        descriptor.vertex.shader = shader.clone();
        let fragment = descriptor.fragment.as_mut().expect("terrain colour");
        fragment.shader = shader;
        ChunkPipelineSpecializer.specialize(key.base, descriptor)?;
        if descriptor.layout.len() != WORLD_MATERIAL_GROUP as usize {
            return Err(BevyError::from(
                "world materials do not draw on Enhanced terrain",
            ));
        }
        descriptor.label = Some("world material terrain pipeline".into());
        descriptor.layout.push(layout(key.world.textures));
        descriptor.vertex.shader_defs.extend(key.world.defs());
        let fragment = descriptor.fragment.as_mut().expect("terrain colour");
        fragment.shader_defs.extend(key.world.defs());
        let target = fragment.targets[0].as_mut().expect("terrain colour target");
        let blended = match key.world.blend {
            Blend::Opaque | Blend::Cutout => None,
            Blend::Premultiplied => Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            Blend::Additive => Some(BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Add,
                },
            }),
        };
        target.blend = blended;
        let depth = descriptor.depth_stencil.as_mut().expect("terrain depth");
        depth.depth_write_enabled = Some(blended.is_none() && key.world.depth == Depth::Test);
        if key.world.depth == Depth::Always {
            depth.depth_compare = Some(CompareFunction::Always);
        }
        Ok(key)
    }
}

/// The fallback pipeline draws every tagged quad as its Bedrock material.
pub(in crate::chunk) fn fallback_descriptor(
    source: &RenderPipelineDescriptor,
) -> RenderPipelineDescriptor {
    let mut descriptor = source.clone();
    descriptor.label = Some("world material fallback terrain pipeline".into());
    descriptor
        .vertex
        .shader_defs
        .push("WORLD_MATERIAL_FALLBACK".into());
    descriptor
}

/// World material slots each section's cube quads carry, by entity and mesh generation, so a
/// section is scanned once per mesh.
#[derive(Resource, Default)]
pub(in crate::chunk) struct WorldTerrainSections {
    masks: HashMap<Entity, (u64, u64)>,
    /// Per view: the sections to draw, with their slot masks.
    views: HashMap<Entity, Vec<(Entity, u64)>>,
}

/// Slot mask of `quads`: bit `tag - 1` for every world material tag present.
fn slot_mask(quads: &[meshing::PackedQuad]) -> u64 {
    quads
        .iter()
        .map(meshing::PackedQuad::world_material)
        .filter(|tag| *tag != 0)
        .fold(0, |mask, tag| mask | (1 << (tag - 1)))
}

/// Queues one world material item per view that sees a tagged section, after resolving each
/// visible section's slot mask.
#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn queue_world_material_chunks(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<ChunkPipeline>,
    world: Option<Res<WorldMaterialGpu>>,
    mut sections: ResMut<WorldTerrainSections>,
    mut opaque_phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    draw_functions: Res<DrawFunctions<Opaque3d>>,
    views: Query<super::draw::ChunkViewQuery>,
    instances: Query<&ChunkRenderInstance>,
    pack: render_scene::shaderpack::ShaderPackDrawnViews,
) {
    let sections = &mut *sections;
    sections.views.clear();
    let Some(world) = world.filter(|world| world.has_materials()) else {
        sections.masks.clear();
        return;
    };
    let draw_function = draw_functions
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
            let Ok(instance) = instances.get(*entity) else {
                continue;
            };
            let mask = match sections.masks.get(entity) {
                Some((generation, mask)) if *generation == instance.generation => *mask,
                _ => {
                    let mask = slot_mask(&instance.cube_quads);
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
        let key = ChunkPipelineKey {
            msaa: *msaa,
            hdr: camera.hdr,
            enhanced: false,
        };
        let union = draws.iter().fold(0, |union, (_, mask)| union | mask);
        for slot in (0..64).filter(|slot| union & (1 << slot) != 0) {
            if let Some(variant) = world.variant(slot) {
                let world_key = WorldTerrainKey {
                    base: key,
                    world: variant,
                };
                if let Ok(id) = pipeline
                    .world_variants
                    .specialize(&pipeline_cache, world_key)
                {
                    pipeline.world_ids.insert(world_key, id);
                }
            }
        }
        let Ok(fallback) = pipeline.world_fallback.specialize(&pipeline_cache, key) else {
            continue;
        };
        let Some(phase) = opaque_phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        phase.add(
            Opaque3dBatchSetKey {
                draw_function,
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
        sections.views.insert(view_entity, draws);
    }
    // Despawned sections leave no stale masks behind.
    sections
        .masks
        .retain(|entity, _| instances.contains(*entity));
}

pub(in crate::chunk) type DrawWorldMaterialTerrainCommands = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    DrawWorldMaterialTerrain,
);

/// Draws a view's tagged sections: each ready material's quads with its pipeline, and every
/// tagged quad of a section with an unready material through the fallback pipeline the item
/// set.
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
        let (Some(bind_group), Some(draws), Some(world)) = (
            &arena.bind_group,
            sections.into_inner().views.get(&view_entity),
            world,
        ) else {
            return RenderCommandResult::Skip;
        };
        let world = world.into_inner();
        let pipeline = pipeline.into_inner();
        let cache = cache.into_inner();
        let key = ChunkPipelineKey {
            msaa: *msaa,
            hdr: camera.hdr,
            enhanced: false,
        };
        let ready = |slot: u16| {
            let (variant, group) = world.slot(slot)?;
            let id = *pipeline.world_ids.get(&WorldTerrainKey {
                base: key,
                world: variant,
            })?;
            Some((cache.get_render_pipeline(id)?, group))
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.set_vertex_buffer(0, arena.vertex_offset_buffer.slice(..));
        let draw = |pass: &mut TrackedRenderPass<'w>, entity: Entity| {
            let Some((cube, _, base_vertex)) =
                allocations.get(entity).ok().and_then(cube_draw_base)
            else {
                return;
            };
            pass.draw_indexed(0..STATIC_QUAD_INDICES.len() as u32, base_vertex, cube);
        };
        // The item's own pipeline is the fallback; sections with an unready material use it.
        let all_ready = |mask: u64| {
            (0..64)
                .filter(|slot| mask & (1 << slot) != 0)
                .all(|slot| ready(slot as u16).is_some())
        };
        for (entity, _) in draws.iter().filter(|(_, mask)| !all_ready(*mask)) {
            draw(pass, *entity);
        }
        let union = draws
            .iter()
            .filter(|(_, mask)| all_ready(*mask))
            .fold(0, |union, (_, mask)| union | mask);
        for slot in (0..64u16).filter(|slot| union & (1 << slot) != 0) {
            let Some((variant, group)) = ready(slot) else {
                continue;
            };
            pass.set_render_pipeline(variant);
            pass.set_bind_group(WORLD_MATERIAL_GROUP as usize, group, &[]);
            for (entity, _) in draws
                .iter()
                .filter(|(_, mask)| mask & (1 << slot) != 0 && all_ready(*mask))
            {
                draw(pass, *entity);
            }
        }
        RenderCommandResult::Success
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_masks_name_every_tagged_material() {
        let quad = |tag| {
            meshing::PackedQuad::new([0; 3], meshing::Face::PositiveY, 1, 1, 1)
                .with_world_material(tag)
        };
        assert_eq!(slot_mask(&[quad(0), quad(0)]), 0);
        assert_eq!(slot_mask(&[quad(1), quad(3), quad(0), quad(3)]), 0b101);
    }

    #[test]
    fn world_terrain_pipelines_own_blend_and_depth_and_swap_both_stages() {
        let base = ChunkPipelineKey {
            msaa: Msaa::Off,
            hdr: false,
            enhanced: false,
        };
        let descriptor = |blend, depth| {
            let mut descriptor = RenderPipelineDescriptor {
                label: None,
                layout: vec![
                    crate::chunk::pipeline::layouts::opaque_chunk_bind_group_layout(),
                    crate::lighting::layout(),
                ],
                vertex: VertexState {
                    shader: CHUNK_SHADER_HANDLE,
                    ..default()
                },
                fragment: Some(FragmentState {
                    shader: CHUNK_SHADER_HANDLE,
                    entry_point: Some("fragment".into()),
                    targets: vec![Some(ColorTargetState {
                        format: render_scene::SCENE_COLOR_FORMAT,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                    ..default()
                }),
                depth_stencil: Some(DepthStencilState {
                    format: CORE_3D_DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(CompareFunction::GreaterEqual),
                    stencil: default(),
                    bias: default(),
                }),
                ..default()
            };
            let world = WorldVariant {
                key: 5,
                textures: 1,
                blend,
                depth,
            };
            WorldTerrainSpecializer
                .specialize(WorldTerrainKey { base, world }, &mut descriptor)
                .unwrap();
            (descriptor, world)
        };
        let (opaque, world) = descriptor(Blend::Cutout, Depth::Test);
        assert_eq!(opaque.vertex.shader, world.shader(TERRAIN_SHADERS));
        assert_eq!(
            opaque.fragment.as_ref().unwrap().shader,
            world.shader(TERRAIN_SHADERS)
        );
        assert_eq!(opaque.layout.len(), 3);
        assert!(!opaque.multisample.alpha_to_coverage_enabled);
        let depth = opaque.depth_stencil.as_ref().unwrap();
        assert_eq!(depth.depth_write_enabled, Some(true));
        let (additive, _) = descriptor(Blend::Additive, Depth::Test);
        assert!(
            additive.fragment.unwrap().targets[0]
                .as_ref()
                .unwrap()
                .blend
                .is_some()
        );
        assert_eq!(
            additive.depth_stencil.unwrap().depth_write_enabled,
            Some(false)
        );
        let (through, _) = descriptor(Blend::Opaque, Depth::Always);
        let depth = through.depth_stencil.unwrap();
        assert_eq!(depth.depth_compare, Some(CompareFunction::Always));
        assert_eq!(depth.depth_write_enabled, Some(false));
    }
}
