//! Many bound blocks cost no more pipelines than one: a published material's pipelines are
//! requested before any bound block is in view, and more bound sections request nothing new.

use super::*;
use bevy::ecs::system::RunSystemOnce;
use bevy::render::render_phase::{AddRenderCommand, DrawFunctions, ViewBinnedRenderPhases};
use bevy::render::view::{RenderVisibleEntities, RenderVisibleEntitiesClass};
use render_scene::world_material::WorldMaterialScene;
use std::sync::Arc;

/// Materials the fixture publishes, one per slot and each its own program.
const MATERIALS: usize = 5;

/// A tagged section: one cube quad per material.
fn section(index: i32) -> ChunkRenderInstance {
    let quads: Vec<_> = (0..MATERIALS as u8)
        .map(|slot| {
            meshing::PackedQuad::new([0; 3], meshing::Face::PositiveY, 1, 1, 1)
                .with_world_material(slot + 1)
        })
        .collect();
    ChunkRenderInstance {
        key: SubChunkKey::new(0, index, 0, 0),
        cube_quads: quads.into(),
        cube_lighting: Arc::from([]),
        cube_layout: Default::default(),
        model_refs: Arc::from([]),
        model_lighting: Arc::from([]),
        model_draw_refs: Arc::from([]),
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
        origin: [index * 16, 0, 0],
        world_materials: false,
    }
}

/// `scene` with each material's source made its own program, so materials share no pipelines.
fn distinct_programs(mut scene: WorldMaterialScene) -> WorldMaterialScene {
    let set = Arc::make_mut(&mut scene.set);
    for (slot, published) in set.materials.iter_mut().flatten().enumerate() {
        let mut material = (*published.material).clone();
        material.source = format!("{}\n// material {slot}", material.source).into();
        published.material = Arc::new(material);
    }
    scene
}

/// A render world publishing [`MATERIALS`] materials, each its own program, with one view.
fn app() -> (
    bevy::app::App,
    Entity,
    bevy::render::view::RetainedViewEntity,
) {
    let (mut app, retained) = crate::queue_review_support::app();
    let identifiers: Vec<String> = (0..MATERIALS).map(|i| format!("example:{i}")).collect();
    let identifiers: Vec<&str> = identifiers.iter().map(String::as_str).collect();
    app.insert_resource(ChunkPipeline::from_world(&mut World::new()))
        .insert_resource(distinct_programs(WorldMaterialScene::with_bound_entities(
            &identifiers,
        )))
        .init_resource::<WorldMaterialGpu>()
        .init_resource::<WorldTerrainSections>()
        .init_resource::<DrawFunctions<Opaque3d>>()
        .init_resource::<ViewBinnedRenderPhases<Opaque3d>>()
        .add_render_command::<Opaque3d, DrawWorldMaterialTerrainCommands>();
    app.world_mut()
        .run_system_once(crate::world_material::prepare)
        .unwrap();
    let view = app
        .world_mut()
        .query_filtered::<Entity, With<ExtractedView>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .entity_mut(view)
        .insert(RenderVisibleEntities::default());
    (app, view, retained)
}

/// Makes `sections` the view's visible chunks and queues one frame.
fn queue(app: &mut bevy::app::App, view: Entity, sections: &[Entity]) {
    let visible = sections
        .iter()
        .map(|&entity| (entity, MainEntity::from(entity)))
        .collect();
    app.world_mut()
        .entity_mut(view)
        .get_mut::<RenderVisibleEntities>()
        .unwrap()
        .classes
        .insert(
            std::any::TypeId::of::<ChunkRenderInstance>(),
            RenderVisibleEntitiesClass {
                entities_cpu_culling: visible,
                ..Default::default()
            },
        );
    app.world_mut()
        .run_system_once(queue_world_material_chunks)
        .unwrap();
}

/// Pipelines the cache was asked for.
fn requested(app: &bevy::app::App) -> usize {
    app.world().resource::<PipelineCache>().pipelines().count()
        + app
            .world()
            .resource::<PipelineCache>()
            .waiting_pipelines()
            .count()
}

#[test]
fn bound_blocks_request_their_pipelines_once_however_many_are_in_view() {
    let (mut app, view, retained) = app();
    app.world_mut()
        .resource_mut::<ViewBinnedRenderPhases<Opaque3d>>()
        .prepare_for_new_frame(
            retained,
            bevy::render::batching::gpu_preprocessing::GpuPreprocessingMode::None,
        );
    // Published but not yet in view: every material's pipelines are already requested.
    queue(&mut app, view, &[]);
    let prewarmed = app.world().resource::<ChunkPipeline>().world_ids.len();
    assert_eq!(
        prewarmed,
        MATERIALS * 2,
        "a cube and a model pipeline per material"
    );
    let before = requested(&app);
    let mut sections = Vec::new();
    for count in [1, 64, 1024] {
        while sections.len() < count {
            let index = sections.len() as i32;
            sections.push(app.world_mut().spawn(section(index)).id());
        }
        queue(&mut app, view, &sections);
        assert_eq!(
            app.world().resource::<ChunkPipeline>().world_ids.len(),
            prewarmed,
            "{count} bound sections"
        );
        assert_eq!(
            requested(&app),
            before,
            "{count} bound sections compile nothing new"
        );
    }
}
