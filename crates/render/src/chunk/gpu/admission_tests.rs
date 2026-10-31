use super::*;
use bevy::ecs::system::RunSystemOnce;
use render_scene::{ChunkRenderInstance, ChunkUploadAcknowledgements, ChunkUploadBudget};

/// Builds an untracked cube upload with valid paired lighting and no external assets.
fn instance() -> ChunkRenderInstance {
    let mesh = solid_test_mesh();
    ChunkRenderInstance {
        key: SubChunkKey::new(0, 0, 0, 0),
        cube_quads: mesh.quads().into(),
        cube_lighting: mesh.cube_lighting().into(),
        cube_layout: mesh.cube_layout(),
        model_refs: Arc::from([]),
        model_lighting: Arc::from([]),
        model_draw_refs: Arc::from([]),
        transparent_model_draw_refs: Arc::from([]),
        liquid_quads: Arc::from([]),
        liquid_lighting: Arc::from([]),
        has_depth_liquid: false,
        has_transparent_liquid: false,
        depth_liquid_start: None,
        biome: PackedBiomeRecord::fallback(),
        tint_identity: ChunkBiomeTintIdentity::default(),
        generation: 1,
        priority: ChunkUploadPriority::new(0.0),
        token: None,
        publication_permit: None,
        origin: [0; 3],
    }
}

/// Runs a frame and returns work counters without relying on elapsed time.
fn prepare(app: &mut App) -> ChunkGpuUploadStats {
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    *app.world().resource::<ChunkGpuUploadStats>()
}

#[test]
fn byte_blocked_backlog_does_no_payload_preparation() {
    let mut app = super::gpu_publication::noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    let template = instance();
    let backlog = 4096;
    app.insert_resource(ChunkUploadBudget::new(
        1,
        chunk_instance_upload_byte_len(&template) - 1,
    ));
    for index in 0..backlog {
        let mut instance = template.clone();
        instance.key = SubChunkKey::new(0, index, 0, 0);
        app.world_mut().spawn(instance);
    }
    let stats = prepare(&mut app);
    eprintln!(
        "blocked backlog={backlog} examined={} validated={} range_plans={} admitted={} deferred={}",
        stats.candidates_examined,
        stats.model_streams_validated,
        stats.range_plans,
        stats.chunk_updates,
        stats.candidates_deferred,
    );
    assert_eq!(stats.chunk_updates, 0);
    assert_eq!(stats.candidates_deferred, backlog as usize);
    assert_eq!(stats.model_streams_validated, 0);
    assert_eq!(stats.range_plans, 0);
    assert!(stats.candidates_examined <= 2 * PublicationServiceConfig::DEFAULT.maximum_frame_items);
}

#[test]
fn allocation_retries_reuse_model_validation() {
    let mut app = super::gpu_publication::noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    app.insert_resource(ChunkUploadBudget::new(1, u64::MAX));
    app.world_mut()
        .resource_mut::<ChunkGpuArena>()
        .limits
        .max_quad_items = 0;
    app.world_mut().spawn(instance());
    let first = prepare(&mut app);
    let retry = prepare(&mut app);
    eprintln!(
        "retry first_validated={} retry_validated={}",
        first.model_streams_validated, retry.model_streams_validated
    );
    assert_eq!(first.model_streams_validated, 1);
    assert_eq!(first.chunk_updates, 0);
    assert_eq!(retry.chunk_updates, 0);
    assert_eq!(retry.model_streams_validated, 0);
}

#[test]
fn blocked_replacement_releases_obsolete_validation_allocations() {
    let mut app = super::gpu_publication::noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    app.insert_resource(ChunkUploadBudget::new(1, u64::MAX));
    let mut instance = instance();
    instance.model_refs = Arc::from([PackedModelRef::new(0, 0, 0, 1)]);
    instance.model_lighting = Arc::from([PackedQuadLighting::new([0; 4])]);
    let refs = Arc::clone(&instance.model_refs);
    let entity = app.world_mut().spawn(instance).id();
    assert_eq!(prepare(&mut app).model_streams_validated, 1);
    assert_eq!(Arc::weak_count(&refs), 1);

    // Replacing one stream drops its last owner; the other streams can stay shared.
    app.world_mut()
        .get_mut::<ChunkRenderInstance>(entity)
        .unwrap()
        .model_lighting = Arc::from([PackedQuadLighting::new([0; 4])]);
    app.insert_resource(ChunkUploadBudget::new(0, 0));
    let stats = prepare(&mut app);
    assert_eq!(stats.model_streams_validated, 0);
    assert_eq!(stats.chunk_updates, 0);
    assert_eq!(Arc::weak_count(&refs), 0);
}

#[test]
fn oversized_prefix_cannot_hide_a_smaller_upload() {
    let mut app = super::gpu_publication::noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    let small = instance();
    app.insert_resource(ChunkUploadBudget::new(
        1,
        chunk_instance_upload_byte_len(&small),
    ));
    let mut large = small.clone();
    large.cube_quads = small.cube_quads.repeat(2).into();
    large.cube_lighting = small.cube_lighting.repeat(2).into();
    let limit = PublicationServiceConfig::DEFAULT.maximum_frame_items;
    let backlog = 8 * limit;
    for index in 0..backlog {
        large.key = SubChunkKey::new(0, index as i32, 0, 0);
        app.world_mut().spawn(large.clone());
    }
    let mut small = small;
    small.key = SubChunkKey::new(0, backlog as i32, 0, 0);
    let entity = app.world_mut().spawn(small).id();
    for _ in 0..backlog.div_ceil(limit) {
        let stats = prepare(&mut app);
        assert!(stats.candidates_examined <= 2 * limit);
        assert!(stats.model_streams_validated <= 1);
        if stats.chunk_updates == 1 {
            assert!(
                app.world()
                    .resource::<ChunkGpuArena>()
                    .allocations
                    .contains_key(&entity)
            );
            return;
        }
    }
    panic!("a byte-blocked prefix hid the fitting upload for a complete scan");
}

#[test]
fn zero_item_budget_skips_model_validation() {
    let mut app = super::gpu_publication::noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    app.insert_resource(ChunkUploadBudget::new(0, u64::MAX));
    app.world_mut().spawn(instance());
    let stats = prepare(&mut app);
    assert_eq!(stats.model_streams_validated, 0);
    assert_eq!(stats.range_plans, 0);
    assert_eq!(stats.chunk_updates, 0);
}

#[test]
fn validation_cache_tracks_every_model_stream_generation_and_asset_identity() {
    use crate::chunk::gpu::upload::model_validation::ModelValidationCache;

    let mut cache = ModelValidationCache::default();
    let mut instance = instance();
    let mut assets = ChunkTextureAssets::default();
    let entity = Entity::from_bits(1);
    let limit = 1;
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (true, true)
    );
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (true, false)
    );
    // A same-generation stream replacement must not reuse a successful verdict.
    instance.model_refs = Arc::from([PackedModelRef::new(0, 0, 0, 1)]);
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, true)
    );
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, false)
    );
    instance.model_lighting = Arc::from([PackedQuadLighting::new([0; 4])]);
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, true)
    );
    instance.model_draw_refs = Arc::from([PackedModelDrawRef::new(0, 0)]);
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, true)
    );
    instance.transparent_model_draw_refs = Arc::from([PackedModelDrawRef::new(0, 0)]);
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, true)
    );
    instance.generation += 1;
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, true)
    );
    assets.revision += 1;
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, true)
    );
    assets.assets = Arc::new(RuntimeAssets::diagnostic());
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, true)
    );
    cache.retain_waiters(&HashMap::new());
    assert_eq!(
        cache.validate(entity, &instance, &assets, limit),
        (false, true)
    );
}

/// The first item always proceeds; later ones stop once the allowance is spent.
#[test]
fn preparation_deadline_admits_one_item_then_stops_when_spent() {
    use crate::chunk::gpu::upload::PreparationDeadline;
    use std::time::Duration;

    let started = web_time::Instant::now();
    let allowance = Duration::from_millis(1);
    let deadline = PreparationDeadline::new(started, allowance);
    assert!(deadline.admits_more(started, 4));
    assert!(deadline.admits_more(started + allowance - Duration::from_nanos(1), 4));
    assert!(!deadline.admits_more(started + allowance, 1));
    assert!(deadline.admits_more(started + allowance * 100, 0));
}

/// Deferred uploads keep their permits and priority order, then acknowledge each generation once.
#[test]
fn spent_preparation_time_defers_the_burst_without_reordering_it() {
    let acknowledgements = ChunkUploadAcknowledgements::default();
    let mut app = super::gpu_publication::noop_gpu_publication_app(
        acknowledgements.clone(),
        ChunkGpuRemovalQueue::default(),
    );
    app.insert_resource(
        ChunkUploadBudget::new(0, 0).with_preparation_time(std::time::Duration::ZERO),
    );
    let config = PublicationServiceConfig::DEFAULT;
    let allowance = render_api::PublicationAllowance::new(config);
    allowance.begin_frame(
        1,
        config.maximum_frame_items,
        config.maximum_frame_bytes,
        0,
        config.maximum_frame_items,
    );
    let template = instance();
    let bytes = chunk_instance_upload_byte_len(&template);
    let now = Instant::now();
    let mut spawn = |x: i32, priority: ChunkUploadPriority| {
        let mut instance = template.clone();
        instance.key = SubChunkKey::new(0, x, 0, 0);
        instance.priority = priority;
        instance.token = Some(render_scene::ChunkUploadToken {
            generation: instance.generation,
            dirty_since: now,
        });
        let permit = allowance
            .try_admit_payload(bytes)
            .unwrap()
            .into_handoff()
            .unwrap()
            .into_render_entity()
            .unwrap();
        instance.publication_permit =
            Some(render_scene::chunk::api::PublicationPermitSlot::new(permit));
        app.world_mut().spawn(instance).id()
    };
    // Spawned out of order: the urgent edit goes first, then nearest first.
    let far = spawn(3, ChunkUploadPriority::new(0.0));
    let near = spawn(0, ChunkUploadPriority::new(0.0));
    let urgent = spawn(5, ChunkUploadPriority::urgent());
    let middle = spawn(1, ChunkUploadPriority::new(0.0));
    let expected = [urgent, near, middle, far];

    let mut admitted = Vec::new();
    for frame in 0..expected.len() {
        let stats = prepare(&mut app);
        assert_eq!(stats.chunk_updates, 1, "frame {frame} admits exactly one");
        assert_eq!(stats.candidates_deferred, expected.len() - frame - 1);
        assert_eq!(stats.time_limited, frame + 1 < expected.len());
        let completed = acknowledgements.drain();
        assert_eq!(completed.len(), 1);
        let uploaded = app
            .world()
            .get::<ChunkRenderInstance>(expected[frame])
            .unwrap();
        assert_eq!(completed[0].key, uploaded.key);
        assert_eq!(completed[0].token, uploaded.token.unwrap());
        assert_eq!(completed[0].uploaded_bytes, bytes);
        let remaining = expected.len() - frame - 1;
        assert_eq!(allowance.live_permits(), remaining);
        assert_eq!(allowance.live_payload_bytes(), bytes * remaining as u64);
        for &entity in &expected[frame + 1..] {
            let deferred = app.world().get::<ChunkRenderInstance>(entity).unwrap();
            assert_eq!(
                deferred.publication_permit.as_ref().unwrap().stage(),
                Some(PublicationPermitStage::RenderEntity),
            );
            assert!(app.world().resource::<GpuUpdateFairness>().wait_age(entity) > 0);
        }
        let arena = app.world().resource::<ChunkGpuArena>();
        let newly = expected
            .iter()
            .copied()
            .filter(|entity| arena.allocations.contains_key(entity) && !admitted.contains(entity))
            .collect::<Vec<_>>();
        admitted.extend(newly);
    }
    assert_eq!(admitted, expected);
    assert_eq!(prepare(&mut app).chunk_updates, 0);
    assert!(acknowledgements.drain().is_empty());
}

/// An unspent allowance leaves the whole burst in one frame, as before the guard.
#[test]
fn unspent_preparation_time_admits_the_whole_burst() {
    let mut app = super::gpu_publication::noop_gpu_publication_app(
        ChunkUploadAcknowledgements::default(),
        ChunkGpuRemovalQueue::default(),
    );
    app.insert_resource(
        ChunkUploadBudget::new(usize::MAX, u64::MAX)
            .with_preparation_time(std::time::Duration::MAX),
    );
    let template = instance();
    for x in 0..4 {
        let mut instance = template.clone();
        instance.key = SubChunkKey::new(0, x, 0, 0);
        app.world_mut().spawn(instance);
    }
    let stats = prepare(&mut app);
    assert_eq!(stats.chunk_updates, 4);
    assert!(!stats.time_limited);
}
