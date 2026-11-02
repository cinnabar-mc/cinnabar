use super::*;
use bevy::prelude::MinimalPlugins;
use render_scene::{ChunkRenderInstance, ChunkRenderQueue, ChunkUploadPriority, ChunkUploadToken};

#[test]
fn cancelled_queued_withdrawal_keeps_the_existing_near_instance() {
    let key = SubChunkKey::new(0, 5, 0, 0);
    let assets = assets::RuntimeAssets::diagnostic();
    let source = world::SubChunk::decode(&[9, 1, 0, 1, 2], &world::RawBlockIds { air: 0 });
    let mesh = meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(0),
        &assets,
        assets::NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &source,
    );
    assert!(!mesh.is_empty());
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(ChunkRenderPlugin::default())
        .insert_resource(FarTerrainScene {
            target: Some(Arc::new(far_terrain::TerrainTarget {
                centre: key.chunk(),
                horizon: 32,
                near: std::sync::Arc::new(HashMap::from([(
                    key.chunk(),
                    HashMap::from([(key, 3)]),
                )])),
                retained: std::sync::Arc::new(HashSet::from([key])),
                empty_far: std::sync::Arc::new(HashSet::new()),
            })),
            ..Default::default()
        });
    let since = Instant::now();
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_update_tracked(
            key,
            mesh,
            ChunkUploadPriority::new(0.0),
            ChunkUploadToken {
                generation: 1,
                dirty_since: since,
            },
        )
        .unwrap();
    app.update();
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_remove_tracked(
            key,
            ChunkUploadPriority::new(0.0),
            ChunkUploadToken {
                generation: 2,
                dirty_since: since,
            },
        )
        .unwrap();
    app.update();
    let world = app.world_mut();
    let mut query = world.query::<&ChunkRenderInstance>();
    let remaining = query
        .single(world)
        .expect("cancelled withdrawal retains the old cover");
    assert_eq!((remaining.key, remaining.generation), (key, 1));
    assert_eq!(world.resource::<ChunkRenderQueue>().retained_len(), 0);
}

/// Without a far target nothing reads empty proofs, so receipts are not copied every frame;
/// a target that arrives later still replays the source's current proofs.
#[test]
fn targetless_frames_leave_empty_proofs_for_the_next_target_to_replay() {
    use bevy::ecs::system::RunSystemOnce;
    let key = SubChunkKey::new(0, 3, 0, 4);
    let source = render_scene::terrain_publication::TerrainPublications::default();
    source.retain(true, [(key.chunk(), vec![key])]);
    let mut app = crate::chunk::tests::gpu_publication::noop_gpu_publication_app(
        render_scene::ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    app.init_resource::<TerrainCoverage>()
        .insert_resource(FarTerrainScene {
            publications: Some(source.clone()),
            ..Default::default()
        });
    let frame = |app: &mut App| {
        let world = app.world_mut();
        world.run_system_once(begin_terrain_coverage).unwrap();
        world.run_system_once(publish_terrain_coverage).unwrap();
    };
    frame(&mut app);
    source.record(key, 2, true);
    frame(&mut app);
    assert_eq!(
        app.world().resource::<TerrainCoverage>().source_revision,
        None
    );

    app.world_mut().resource_mut::<FarTerrainScene>().target =
        Some(Arc::new(far_terrain::TerrainTarget {
            centre: key.chunk(),
            horizon: 32,
            near: Arc::new(HashMap::from([(key.chunk(), HashMap::from([(key, 2)]))])),
            retained: Arc::new(HashSet::from([key])),
            empty_far: Arc::new(HashSet::new()),
        }));
    frame(&mut app);
    let (revision, empty) = source.changed_empty(None).unwrap();
    assert_eq!(empty, HashMap::from([(key, 2)]));
    assert_eq!(
        app.world().resource::<TerrainCoverage>().source_revision,
        Some(revision)
    );
}

/// Both drawers stay admitted and unretirable for the whole fade, in both directions.
#[test]
fn near_and_far_handover_retain_allocations_and_publish_masks_together() {
    use bevy::ecs::system::RunSystemOnce;
    use render_scene::{ChunkUploadAcknowledgements, ChunkUploadBudget};
    let now = std::time::Duration::from_secs(10);
    let key = SubChunkKey::new(0, 0, 0, 0);
    let mut producer = App::new();
    producer
        .add_plugins(MinimalPlugins)
        .insert_resource(ChunkRenderQueue::default())
        .add_plugins(ChunkRenderPlugin::default());
    producer
        .world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_update_tracked(
            key,
            crate::chunk::tests::solid_test_mesh(),
            ChunkUploadPriority::new(0.0),
            ChunkUploadToken {
                generation: 1,
                dirty_since: Instant::now(),
            },
        )
        .unwrap();
    producer.update();
    let instance = {
        let world = producer.world_mut();
        world
            .query::<&ChunkRenderInstance>()
            .single(world)
            .unwrap()
            .clone()
    };
    let mut app = crate::chunk::tests::gpu_publication::noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    let device = app.world().resource::<RenderDevice>().clone();
    app.insert_resource(crate::far_terrain::FarTerrainGpu::new(&device))
        .insert_resource(ChunkUploadBudget::new(8, u64::MAX))
        .init_resource::<TerrainCoverage>();
    let entity = app.world_mut().spawn(instance).id();
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    let tile = far_terrain::TileKey {
        level: 0,
        x: 0,
        z: 0,
    };
    let mut summary = far_terrain::ColumnSummary::default();
    summary.cells_mut().fill(far_terrain::SurfaceCell {
        height: 16,
        rgb: [80; 3],
        water_depth: 0,
    });
    let mesh = Arc::new(far_terrain::mesh_tile(&far_terrain::TileGrid::build(
        tile,
        &tile
            .columns()
            .map(|column| (column, summary.clone()))
            .collect::<Vec<_>>(),
    )));
    let target = |near| {
        Arc::new(far_terrain::TerrainTarget {
            centre: key.chunk(),
            horizon: 32,
            near: Arc::new(if near {
                HashMap::from([(key.chunk(), HashMap::from([(key, 1)]))])
            } else {
                HashMap::new()
            }),
            retained: Arc::new(HashSet::from([key])),
            empty_far: Arc::new(HashSet::new()),
        })
    };
    app.insert_resource(FarTerrainScene {
        target: Some(target(true)),
        tiles: Arc::from([mesh]),
        ..Default::default()
    });
    app.world_mut()
        .run_system_once(begin_terrain_coverage)
        .unwrap();
    app.world_mut()
        .run_system_once(crate::far_terrain::gpu::prepare_far_terrain)
        .unwrap();
    app.insert_resource(Time::<()>::default());
    let publish = |app: &mut App, at| {
        app.world_mut().resource_mut::<Time>().advance_to(at);
        app.world_mut()
            .run_system_once(publish_terrain_coverage)
            .unwrap();
    };
    let column = ColumnPos::new(key.x, key.z);
    publish(&mut app, now);
    assert!(
        app.world()
            .resource::<TerrainCoverage>()
            .mask
            .as_ref()
            .unwrap()
            .contains(column)
    );
    app.world_mut().resource_mut::<FarTerrainScene>().target = Some(target(false));
    publish(&mut app, now);
    assert!(app.world().get::<GpuChunkAllocation>(entity).is_some());
    assert!(
        !app.world()
            .resource::<FarTerrainScene>()
            .residency
            .may_remove_near(column)
    );
    assert!(
        !app.world()
            .resource::<TerrainCoverage>()
            .mask
            .as_ref()
            .unwrap()
            .contains(column)
    );
    publish(&mut app, now + far_terrain::HANDOFF_DURATION / 2);
    assert!(app.world().get::<GpuChunkAllocation>(entity).is_some());
    publish(&mut app, now + far_terrain::HANDOFF_DURATION);
    assert!(app.world().get::<GpuChunkAllocation>(entity).is_none());
    assert!(
        app.world()
            .resource::<FarTerrainScene>()
            .residency
            .may_remove_near(column)
    );
    let back = now + far_terrain::HANDOFF_DURATION;
    app.world_mut().resource_mut::<FarTerrainScene>().target = Some(target(true));
    publish(&mut app, back);
    assert!(app.world().get::<GpuChunkAllocation>(entity).is_some());
    assert!(
        !app.world()
            .resource::<TerrainCoverage>()
            .mask
            .as_ref()
            .unwrap()
            .contains(column)
    );
    publish(&mut app, back + far_terrain::HANDOFF_DURATION);
    assert!(
        app.world()
            .resource::<TerrainCoverage>()
            .mask
            .as_ref()
            .unwrap()
            .contains(column)
    );
    assert!(app.world().get::<GpuChunkAllocation>(entity).is_some());
}
