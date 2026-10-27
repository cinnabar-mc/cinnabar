//! Drawing sections whose bound translucent faces merge into their ordinary translucent order.
//! Such a section draws as one sorted item instead of its ordinary model, water or mixed items:
//! each run of its merged order draws with the ordinary translucent pipeline or with its world
//! material's pipeline.

use super::draw::ready;
use super::interleave::{
    OrderIdentity, SectionFaces, Segment, Stream, WorldList, collect_faces, order,
};
use super::*;
use crate::chunk::transparent::face_metric::TransparentFaceMetric;
use bevy::render::render_resource::CachedRenderPipelineId;
use std::collections::HashMap;
use std::sync::Arc;

/// One merged section draw of this frame.
struct InterleavedDraw {
    view_entity: Entity,
    section: Entity,
    /// The ordinary translucent pipeline of the view.
    pipeline: CachedRenderPipelineId,
    /// The model order revision the model indices name, checked again when drawing.
    model: Option<(TransparentModelAllocationIdentity, Option<u64>)>,
    segments: Arc<[Segment]>,
}

/// One view's merged order of one section and what it was computed from.
type CachedOrder = (OrderIdentity, Arc<[Segment]>);

/// This frame's merged section draws, and each view's section orders kept while their inputs
/// are unchanged.
#[derive(Resource, Default)]
pub(in crate::chunk) struct InterleavedTerrain {
    frame: Vec<InterleavedDraw>,
    cache: HashMap<(Entity, Entity), CachedOrder>,
    retained: HashMap<(Entity, Entity), CachedOrder>,
}

impl InterleavedTerrain {
    /// Starts a frame: last frame's orders stay available to reuse, and only orders this frame
    /// plans are kept afterwards.
    pub(in crate::chunk) fn begin_frame(&mut self) {
        self.frame.clear();
        self.retained = std::mem::take(&mut self.cache);
    }

    /// Plans `section`'s merged order for `view_entity` and returns its draw index, or `None`
    /// when the section is no longer resident or its model order cannot be read.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::chunk) fn plan(
        &mut self,
        view_entity: Entity,
        camera: Vec3,
        section: Entity,
        slots: u64,
        instance: &ChunkRenderInstance,
        allocation: &GpuChunkAllocation,
        models: &TransparentModelSortRuntime,
        assets: &render_scene::ChunkTextureAssets,
        pipeline: CachedRenderPipelineId,
    ) -> Option<u32> {
        camera.is_finite().then_some(())?;
        let model = allocation
            .model_range
            .clone()
            .zip(allocation.transparent_model_draw_range.clone())
            .map(
                |(model_range, draw_range)| TransparentModelAllocationIdentity {
                    entity: section,
                    key: allocation.key,
                    generation: allocation.generation,
                    model_range,
                    draw_range,
                },
            );
        let model_base = allocation
            .model_range
            .as_ref()
            .map_or(0, |range| range.start / 4);
        let published = model
            .as_ref()
            .and_then(|identity| models.draw_orders.get(identity));
        let identity = OrderIdentity {
            generation: instance.generation,
            assets: assets.identity(),
            class: TransparentFaceMetric::new(camera).class(instance.key),
            model_revision: published.map(|order| order.revision),
            slots,
        };
        let key = (view_entity, section);
        let segments = match self.retained.remove(&key) {
            Some((cached, segments)) if cached == identity => segments,
            _ => {
                // Until a sorted order is uploaded the GPU holds the mesher's order.
                let mesh_order;
                let model_words = match published {
                    Some(order) => &order.words[..],
                    None => {
                        mesh_order = instance
                            .transparent_model_draw_refs
                            .iter()
                            .map(|draw| {
                                let [local, quad] = draw.words();
                                [local + model_base, quad]
                            })
                            .collect::<Vec<_>>();
                        &mesh_order[..]
                    }
                };
                let runtime = assets.assets();
                order(
                    instance.key,
                    camera,
                    collect_faces(&SectionFaces {
                        instance,
                        model_words,
                        model_base,
                        templates: runtime.model_templates(),
                        model_quads: runtime.model_quads(),
                        slots,
                    }),
                )
                .into()
            }
        };
        let revision = identity.model_revision;
        self.cache.insert(key, (identity, Arc::clone(&segments)));
        let index = u32::try_from(self.frame.len()).ok()?;
        self.frame.push(InterleavedDraw {
            view_entity,
            section,
            pipeline,
            model: model.map(|identity| (identity, revision)),
            segments,
        });
        Some(index)
    }
}

pub(in crate::chunk) type DrawInterleavedTerrainCommands =
    (crate::lighting::SetWorldLightmap, DrawInterleavedTerrain);

/// Draws one section's merged translucent order, switching between the ordinary translucent
/// pipeline and its world materials' pipelines at each run.
pub(in crate::chunk) struct DrawInterleavedTerrain;

impl RenderCommand<Transparent3d> for DrawInterleavedTerrain {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<PipelineCache>,
        SRes<InterleavedTerrain>,
        Option<SRes<WorldMaterialGpu>>,
        SRes<ChunkPipeline>,
        SRes<TransparentModelSortRuntime>,
        Option<SRes<crate::world_material::scene::WorldSceneCopies>>,
    );
    type ViewQuery = (
        Entity,
        Read<ViewUniformOffset>,
        Read<Msaa>,
        Read<bevy::render::camera::ExtractedCamera>,
    );
    type ItemQuery = Read<GpuChunkAllocation>;

    fn render<'w>(
        item: &Transparent3d,
        (view_entity, view_offset, msaa, camera): ROQueryItem<'w, '_, Self::ViewQuery>,
        allocation: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, cache, interleaved, world, pipeline, models, scenes): SystemParamItem<
            'w,
            '_,
            Self::Param,
        >,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (arena, cache, interleaved, pipeline) = (
            arena.into_inner(),
            cache.into_inner(),
            interleaved.into_inner(),
            pipeline.into_inner(),
        );
        let scene = scenes.and_then(|scenes| scenes.into_inner().group(view_entity));
        let PhaseItemExtraIndex::IndirectParametersIndex { range, .. } = item.extra_index() else {
            return RenderCommandResult::Skip;
        };
        let (Some(draw), Some(allocation), Some(world)) = (
            interleaved.frame.get(range.start as usize),
            allocation,
            world,
        ) else {
            return RenderCommandResult::Skip;
        };
        let world = world.into_inner();
        // The model indices name the order uploaded when this frame planned it.
        let current = draw
            .model
            .as_ref()
            .map(|(identity, _)| models.draw_orders.get(identity).map(|order| order.revision));
        if draw.view_entity != view_entity
            || draw.section != item.entity()
            || current != draw.model.as_ref().map(|(_, revision)| *revision)
        {
            return RenderCommandResult::Skip;
        }
        let (Some(ordinary), Some(transparent_group), Some(world_group)) = (
            cache.get_render_pipeline(draw.pipeline),
            &arena.transparent_bind_group,
            &arena.bind_group,
        ) else {
            return RenderCommandResult::Skip;
        };
        let base = ChunkPipelineKey {
            msaa: *msaa,
            hdr: camera.hdr,
            enhanced: false,
        };
        let cubes = cube_draw_base(allocation);
        let models_args = [
            model_direct_draw_command(allocation),
            transparent_model_direct_draw_command(allocation),
        ];
        let water = transparent_liquid_direct_draw_command(allocation);
        pass.set_vertex_buffer(0, arena.vertex_offset_buffer.slice(..));
        for segment in draw.segments.iter() {
            let instances = |first: u32, count: u32| {
                (segment.range.end <= count)
                    .then(|| first + segment.range.start..first + segment.range.end)
            };
            match segment.stream {
                Stream::Model | Stream::Water => {
                    let args = match segment.stream {
                        Stream::Model => models_args[1],
                        _ => water,
                    };
                    let Some(args) = args else { continue };
                    let Some(instances) = instances(args.first_instance, args.instance_count)
                    else {
                        continue;
                    };
                    pass.set_render_pipeline(ordinary);
                    pass.set_bind_group(0, transparent_group, &[view_offset.offset]);
                    pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
                    pass.draw_indexed(
                        args.first_index..args.first_index + args.index_count,
                        args.base_vertex,
                        instances,
                    );
                }
                Stream::World { slot, list } => {
                    let geometry = match list {
                        WorldList::Cube => Geometry::Cube,
                        WorldList::Model | WorldList::TransparentModel => Geometry::Model,
                    };
                    let Some((world_pipeline, material_group, scene_group, _)) =
                        ready(slot, (world, scene), pipeline, cache, base, geometry)
                    else {
                        continue;
                    };
                    let (indices, base_vertex, instances, buffer) = match list {
                        WorldList::Cube => {
                            let Some((quads, _, base_vertex)) = &cubes else {
                                continue;
                            };
                            let Some(instances) = instances(quads.start, quads.end - quads.start)
                            else {
                                continue;
                            };
                            (
                                0..STATIC_QUAD_INDICES.len() as u32,
                                *base_vertex,
                                instances,
                                &arena.index_buffer,
                            )
                        }
                        WorldList::Model | WorldList::TransparentModel => {
                            let list = usize::from(list == WorldList::TransparentModel);
                            let Some(args) = models_args[list] else {
                                continue;
                            };
                            let Some(instances) =
                                instances(args.first_instance, args.instance_count)
                            else {
                                continue;
                            };
                            (
                                args.first_index..args.first_index + args.index_count,
                                args.base_vertex,
                                instances,
                                &arena.model_index_buffer,
                            )
                        }
                    };
                    pass.set_render_pipeline(world_pipeline);
                    pass.set_bind_group(0, world_group, &[view_offset.offset]);
                    pass.set_bind_group(WORLD_MATERIAL_GROUP as usize, material_group, &[]);
                    if let Some(scene_group) = scene_group {
                        pass.set_bind_group(WORLD_MATERIAL_GROUP as usize + 1, scene_group, &[]);
                    }
                    pass.set_index_buffer(buffer.slice(..), IndexFormat::Uint32);
                    pass.draw_indexed(indices, base_vertex, instances);
                }
            }
        }
        RenderCommandResult::Success
    }
}
