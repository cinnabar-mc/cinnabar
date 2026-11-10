use super::*;

/// Queues a visible mesh outside local probes ahead of a lower-priority ready generation.
fn deferred_mesh_stream() -> (WorldStream, SchedulerView, SubChunkKey, SubChunkKey) {
    seed_deferred_meshes(lit_stream(1))
}

/// Repeated light notifications share one readiness check until geometry can be scheduled.
#[test]
fn completed_light_notifications_coalesce_readiness() {
    let (mut stream, view, visible, _) = deferred_mesh_stream();
    let finished = SubChunkKey::new(1, visible.x + 1, 1, 1);
    let blocked = SubChunkKey::new(1, visible.x - 1, -1, -1);
    install_current_light(&mut stream, finished, 0, 0, false);
    install_current_light(&mut stream, blocked, 0, 0, false);
    stream.mark_light_dirty_exact(blocked).unwrap();
    let direct = stream.lighting.direct_sky[&finished].clone();
    meshing::MESH_READINESS_CHECKS.with(|checks| checks.set(0));
    for _ in 0..32 {
        stream.finish_accepted_light_completion(finished, 0, &direct, [false; 6], false);
    }
    let checks = meshing::MESH_READINESS_CHECKS.with(std::cell::Cell::get);
    assert!(
        checks <= 1,
        "repeated unresolved notifications performed {checks} readiness checks"
    );
    assert!(!stream.mesh_jobs.in_flight.contains_key(&visible));

    install_current_light(&mut stream, blocked, 0, 0, false);
    let direct = stream.lighting.direct_sky[&blocked].clone();
    stream.finish_accepted_light_completion(blocked, 0, &direct, [false; 6], false);
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, visible);
    stream.accept_mesh_completion(completion);
}

/// Worker saturation retains dependency notifications without doing geometry preparation.
#[test]
fn completed_light_notifications_wait_for_geometry_capacity() {
    let (mut stream, view, visible, _) = deferred_mesh_stream();
    let finished = SubChunkKey::new(1, visible.x + 1, 1, 1);
    install_current_light(&mut stream, finished, 0, 0, false);
    let direct = stream.lighting.direct_sky[&finished].clone();
    stream.finish_accepted_light_completion(finished, 0, &direct, [false; 6], false);
    stream
        .admitted_mesh_jobs
        .store(meshing::admission::mesh_worker_cap(), Ordering::Release);
    meshing::MESH_READINESS_CHECKS.with(|checks| checks.set(0));
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 0);
    assert_eq!(meshing::MESH_READINESS_CHECKS.with(std::cell::Cell::get), 0);
    stream.admitted_mesh_jobs.store(0, Ordering::Release);
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, visible);
    stream.accept_mesh_completion(completion);
}

/// A retired revision cannot leave a dependency notification ahead of current ready work.
#[test]
fn retired_mesh_releases_coalesced_dependency_notification() {
    let (mut stream, view, visible, backlog) = deferred_mesh_stream();
    let finished = SubChunkKey::new(1, visible.x + 1, 1, 1);
    install_current_light(&mut stream, finished, 0, 0, false);
    let direct = stream.lighting.direct_sky[&finished].clone();
    for _ in 0..32 {
        stream.finish_accepted_light_completion(finished, 0, &direct, [false; 6], false);
    }
    assert!(
        stream.mesh_jobs.lanes[RESIDENT_MESH_LANE]
            .notified
            .get(&visible)
            .is_some()
    );
    stream.mesh_jobs.remove_pending(&visible);
    assert!(
        stream.mesh_jobs.lanes[RESIDENT_MESH_LANE]
            .notified
            .get(&visible)
            .is_none()
    );
    meshing::MESH_READINESS_CHECKS.with(|checks| checks.set(0));
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, backlog);
    assert_eq!(meshing::MESH_READINESS_CHECKS.with(std::cell::Cell::get), 0);
    stream.accept_mesh_completion(completion);
}

/// A lower-ranked dependency change must not spend preparation ahead of ready geometry.
#[test]
fn ready_geometry_precedes_lower_rank_dependency_preparation() {
    let (mut stream, _, visible, backlog) = deferred_mesh_stream();
    let finished = SubChunkKey::new(1, visible.x + 1, 1, 1);
    install_current_light(&mut stream, finished, 0, 0, false);
    stream.mark_light_dirty_exact(visible).unwrap();
    let direct = stream.lighting.direct_sky[&finished].clone();
    stream.finish_accepted_light_completion(finished, 0, &direct, [false; 6], false);
    meshing::MESH_READINESS_CHECKS.with(|checks| checks.set(0));
    let camera = [(backlog.x * 16 + 8) as f32, 8.0, 8.0];
    assert_eq!(stream.dispatch_mesh_jobs(camera, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, backlog);
    assert_eq!(meshing::MESH_READINESS_CHECKS.with(std::cell::Cell::get), 0);
    stream.accept_mesh_completion(completion);
}

/// Queues a visible far mesh ahead of lower-priority work in a dimension-one test stream.
fn seed_deferred_meshes(
    mut stream: WorldStream,
) -> (WorldStream, SchedulerView, SubChunkKey, SubChunkKey) {
    stream.set_view_forward([1.0, 0.0, 0.0]);
    let view = stream.scheduler_view([8.0; 3]);
    assert!(view.startup_center.is_none());
    stream
        .mesh_jobs
        .refresh
        .refresh(view, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    let visible = SubChunkKey::new(1, scheduler::NEAR_CAMERA_RADIUS * 3, 0, 0);
    let backlog = SubChunkKey::new(1, visible.x + 4, 0, 0);
    assert!(!scheduler::mesh_is_near(view, visible));
    let ready = (0..8).map(|offset| {
        (
            SubChunkKey::new(1, backlog.x + offset * 2, 0, 0),
            offset != 7,
        )
    });
    for (key, ready) in std::iter::once((visible, false)).chain(ready) {
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        let revision = stream.mark_dirty_exact(key, Instant::now());
        let candidate = PendingSchedulerCandidate::new(key, revision, view, false);
        let lane = &mut stream.mesh_jobs.lanes[RESIDENT_MESH_LANE];
        if ready {
            lane.ready.push(candidate);
        } else {
            lane.deferred.push(candidate);
        }
    }
    stream.mesh_jobs.scan.clear();
    (stream, view, visible, backlog)
}

/// A settled far halo must publish its higher-priority mesh before the ready generation drains.
#[test]
fn far_settled_halo_publishes_before_ready_backlog() {
    let (mut stream, view, visible, backlog) = deferred_mesh_stream();
    let corner = SubChunkKey::new(1, visible.x + 1, visible.y + 1, visible.z + 1);
    install_current_light(&mut stream, corner, 0, 0, false);
    stream.mark_light_dirty_exact(corner).unwrap();
    assert!(stream.mesh_light_halo(visible).is_none());
    let revision = stream.mesh_jobs.pending[&visible].revision;

    install_current_light(&mut stream, corner, 0, 0, false);
    let direct = stream.lighting.direct_sky[&corner].clone();
    stream.finish_accepted_light_completion(corner, 0, &direct, [false; 6], false);
    assert_eq!(stream.mesh_jobs.pending[&visible].revision, revision);
    assert!(stream.mesh_jobs.scan.is_empty());

    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, visible);
    assert_eq!(completion.revision, revision);
    stream.accept_mesh_completion(completion);
    assert!(
        stream
            .take_mesh_changes()
            .iter()
            .any(|change| change.key() == visible)
    );
    assert!(stream.mesh_jobs.pending.contains_key(&backlog));
}

/// A completed halo wakes distant geometry without waiting behind unrelated blocked retries.
#[test]
fn far_completed_halo_bypasses_blocked_retry_rotation() {
    for refreshing in [false, true] {
        let mut initial = lit_stream(1);
        let radius = scheduler::NEAR_CAMERA_RADIUS;
        // Keep the stale backlog behind the camera and outside its direct probes.
        for index in 0..MAX_PENDING_SCHEDULER_SCANS_PER_POLL * 2 {
            let index = i32::try_from(index).unwrap();
            let key = SubChunkKey::new(
                1,
                -radius - 1 - index % (radius * 2),
                index / (radius * 2) % radius,
                index / (radius * radius * 2) - radius,
            );
            initial
                .authority
                .commit_sub_chunk(key, super::uniform_sub_chunk(2))
                .unwrap();
            install_current_light(&mut initial, key, 0, 0, false);
            initial.mark_light_dirty_exact(key).unwrap();
            initial.mark_dirty_exact(key, Instant::now());
        }
        let (mut stream, mut view, visible, backlog) = seed_deferred_meshes(initial);
        if refreshing {
            view.position[2] += 32.0;
            stream.mesh_jobs.refresh.refresh(
                view,
                &mut stream.mesh_jobs.lanes,
                Some(Instant::now()),
                |_, _| true,
            );
        }
        let corner = SubChunkKey::new(1, visible.x + 1, visible.y + 1, visible.z + 1);
        install_current_light(&mut stream, corner, 0, 0, false);
        let light_completion = synthetic_light_completion(
            &mut stream,
            corner,
            DirectSkyMask::Uniform(false),
            false,
            false,
            [false; 6],
        );
        assert!(stream.mesh_light_halo(visible).is_none());
        let pending = stream.mesh_jobs.pending[&visible];

        stream.accept_light_completion(light_completion);
        assert!(stream.mesh_light_halo(visible).is_some());
        assert_eq!(
            stream.mesh_jobs.pending[&visible].revision,
            pending.revision
        );
        assert_eq!(
            stream.mesh_jobs.pending[&visible].queued_at,
            pending.queued_at
        );

        stream.poll_deadline = Some(Instant::now());
        assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
        let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(completion.key, visible);
        assert_eq!(completion.revision, pending.revision);
        stream.accept_mesh_completion(completion);
        assert!(
            stream
                .take_mesh_changes()
                .iter()
                .any(|change| change.key() == visible)
        );
        assert!(stream.mesh_jobs.pending.contains_key(&backlog));
    }
}

/// A successful retry spends the expired poll's single dispatch allowance.
#[test]
fn expired_far_retry_dispatches_once_before_ready_backlog() {
    let (mut stream, view, visible, backlog) = deferred_mesh_stream();
    stream.poll_deadline = Some(Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 2), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, visible);
    stream.accept_mesh_completion(completion);
    assert!(stream.mesh_jobs.pending.contains_key(&backlog));
    assert!(!stream.mesh_jobs.in_flight.contains_key(&backlog));
}

/// Rechecking a far mesh must keep stale light blocked and let ready geometry progress.
#[test]
fn far_stale_halo_cannot_starve_ready_backlog() {
    let (mut stream, view, visible, backlog) = deferred_mesh_stream();
    stream.mark_light_dirty_exact(visible).unwrap();
    for _ in 0..4 {
        stream.poll_deadline = Some(Instant::now());
        stream.dispatch_mesh_jobs(view.position, 1);
        if stream.mesh_jobs.in_flight.contains_key(&backlog) {
            break;
        }
    }
    assert!(!stream.mesh_jobs.in_flight.contains_key(&visible));
    assert!(stream.mesh_jobs.pending.contains_key(&visible));
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, backlog);
    stream.accept_mesh_completion(completion);
}

/// Deferred ordinary work yields to urgent edits and destination footing.
#[test]
fn far_deferred_mesh_preserves_urgent_and_transfer_precedence() {
    for transfer in [false, true] {
        let (mut stream, view, visible, backlog) = deferred_mesh_stream();
        if transfer {
            let side = world::SUB_CHUNK_SIDE as f32;
            stream.set_dimension_transfer_priority(Some([
                backlog.x as f32 * side + side / 2.0,
                client_world::ingestion::PLAYER_NETWORK_OFFSET + side / 2.0,
                side / 2.0,
            ]));
        } else {
            stream.mark_dirty_exact_with_priority(backlog, Instant::now(), true);
        }
        assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
        let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(completion.key, backlog);
        stream.accept_mesh_completion(completion);
        assert!(stream.mesh_jobs.pending.contains_key(&visible));
    }
}

/// A due neighbour can settle without revising the mesh that was waiting for it.
#[test]
fn far_settled_neighbour_publishes_before_ready_backlog() {
    let (mut stream, view, visible, backlog) = deferred_mesh_stream();
    let missing = SubChunkKey::new(1, visible.x + 1, visible.y, visible.z);
    let now = Instant::now();
    stream
        .unsent_column_deadlines
        .insert(missing.chunk(), now + UNSENT_COLUMN_GRACE);
    assert!(stream.mesh_neighbour_is_due(visible, now));
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, backlog);
    stream.accept_mesh_completion(completion);
    stream.take_mesh_changes();
    let backlog = SubChunkKey::new(1, backlog.x + 2, backlog.y, backlog.z);
    assert!(stream.mesh_jobs.pending.contains_key(&visible));
    let revision = stream.mesh_jobs.pending[&visible].revision;

    stream.loaded_columns.insert(missing.chunk());
    assert!(!stream.mesh_neighbour_is_due(visible, now));
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, visible);
    assert_eq!(completion.revision, revision);
    stream.accept_mesh_completion(completion);
    assert!(
        stream
            .take_mesh_changes()
            .iter()
            .any(|change| change.key() == visible)
    );
    assert!(stream.mesh_jobs.pending.contains_key(&backlog));
}

/// Continuous arrivals between a settled retry and ready work cannot hide that retry.
#[test]
fn far_settled_retry_precedes_continuous_fresh_ingress() {
    let (mut stream, view, visible, backlog) = deferred_mesh_stream();
    stream.mark_light_dirty_exact(visible).unwrap();
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, backlog);
    stream.accept_mesh_completion(completion);
    stream.take_mesh_changes();
    install_current_light(&mut stream, visible, 0, 0, false);
    let backlog = SubChunkKey::new(1, backlog.x + 2, backlog.y, backlog.z);

    let mut published = false;
    for z in 0..8 {
        let arriving = SubChunkKey::new(1, visible.x + 2, 0, z);
        stream
            .authority
            .commit_sub_chunk(arriving, super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, arriving, 0, 0, false);
        stream.mark_dirty_exact(arriving, Instant::now());
        assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
        let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        published = completion.key == visible;
        stream.accept_mesh_completion(completion);
        if published {
            break;
        }
    }
    assert!(
        published,
        "fresh lower-priority ingress hid the settled retry"
    );
    assert!(stream.mesh_jobs.pending.contains_key(&backlog));
}

/// Output-memory pressure must not starve a smaller ready mesh or duplicate retry heap records.
#[test]
fn memory_blocked_retry_keeps_ready_progress_and_bounded_storage() {
    assert_memory_blocked_retry_progress(1, false);
}

/// More blocked retries than the cooldown must still leave ordinary work an admission turn.
#[test]
fn many_memory_blocked_retries_preserve_ready_progress_across_rotations() {
    let blocked = usize::try_from(scheduler::MAX_RECHECK_BACKOFF_POLLS).unwrap() + 1;
    assert_memory_blocked_retry_progress(blocked, false);
}

/// Unchanged light completions must not repeatedly promote a mesh that cannot fit its output.
#[test]
fn light_completion_preserves_memory_backoff() {
    assert_memory_blocked_retry_progress(1, true);
}

/// Keeps smaller ready meshes publishing while a FIFO of larger models cannot reserve output.
fn assert_memory_blocked_retry_progress(blocked: usize, wake: bool) {
    let base = light_test_assets();
    let model_id = u32::try_from(base.visual_count()).unwrap();
    let side = assets::MODEL_POSITION_SCALE;
    let uv = assets::MODEL_UV_SCALE;
    let overlay = assets::BlockOverlay {
        visuals: vec![BlockVisual {
            native_material: assets::native_material::NativeMaterialFacts::Unresolved,
            support: assets::VisualSupport::Exact,
            faces: [0; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Model,
            contributor_role: ContributorRole::Primary,
            model_template: 0,
            animation: NO_ANIMATION,
            variant: 0,
            destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
        }],
        light_properties: vec![LightProperties::new(0, 0).unwrap()],
        materials: vec![Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
            ..Material::unvaried()
        }],
        model_templates: vec![assets::ModelTemplate {
            quad_start: 0,
            quad_count: 1,
            flags: 0,
        }],
        model_quads: vec![assets::ModelQuad {
            positions: [[0, 0, 0], [0, side, 0], [side, side, 0], [side, 0, 0]],
            uvs: [[0, 0], [0, uv], [uv, uv], [uv, 0]],
            material: 0,
            flags: 0,
        }],
        ..assets::BlockOverlay::default()
    };
    let assets = base.with_block_overlay(model_id, &overlay).unwrap();
    let mut stream = lit_stream_with_assets(1, assets);
    stream.set_view_forward([1.0, 0.0, 0.0]);
    let view = stream.scheduler_view([8.0; 3]);
    stream
        .mesh_jobs
        .refresh
        .refresh(view, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    let visible = SubChunkKey::new(1, scheduler::NEAR_CAMERA_RADIUS * 3, 0, 0);
    let backlog = SubChunkKey::new(1, visible.x + i32::try_from(blocked).unwrap() * 2 + 4, 0, 0);
    for offset in 0..=blocked {
        let ready = offset == blocked;
        let key = if ready {
            backlog
        } else {
            SubChunkKey::new(1, visible.x + i32::try_from(offset).unwrap() * 2, 0, 0)
        };
        stream
            .authority
            .commit_sub_chunk(
                key,
                super::uniform_sub_chunk(if ready { 2 } else { model_id }),
            )
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        let revision = stream.mark_dirty_exact(key, Instant::now());
        let candidate = PendingSchedulerCandidate::new(key, revision, view, false);
        let lane = &mut stream.mesh_jobs.lanes[RESIDENT_MESH_LANE];
        if ready {
            lane.ready.push(candidate);
        } else {
            lane.deferred.push(candidate);
        }
    }
    stream.mesh_jobs.scan.clear();
    let revision = stream.mesh_jobs.pending[&visible].revision;
    let source = stream.authority.terrain().sub_chunk(visible).unwrap();
    let small = stream.authority.terrain().sub_chunk(backlog).unwrap();
    let reserve = |source| {
        stream
            .mesh_memory
            .try_admit(
                source,
                stream.authority.runtime_assets(),
                stream.authority.network_id_mode(),
                0,
            )
            .unwrap()
    };
    let large_permit = reserve(&source);
    let large_bytes = stream.mesh_memory.retained.load(Ordering::Acquire);
    drop(large_permit);
    let small_permit = reserve(&small);
    let small_bytes = stream.mesh_memory.retained.load(Ordering::Acquire);
    drop(small_permit);
    assert!(large_bytes > small_bytes);
    let held = crate::stream::meshing::memory::MAX_MESH_OUTPUT_BYTES - small_bytes;
    stream.mesh_memory.retained.store(held, Ordering::Release);
    assert!(
        stream
            .mesh_memory
            .try_admit(
                &source,
                stream.authority.runtime_assets(),
                stream.authority.network_id_mode(),
                0,
            )
            .is_none()
    );
    drop(reserve(&small));
    let lane = &stream.mesh_jobs.lanes[RESIDENT_MESH_LANE];
    let storage_limit = lane.ready.len() + lane.deferred.len() + stream.mesh_jobs.scan.len();
    let mut published = 0;
    for _ in 0..(blocked * 3).max(12) {
        if !stream.mesh_jobs.pending.contains_key(&backlog) {
            stream.mark_dirty_exact(backlog, Instant::now());
        }
        if wake {
            let completion = synthetic_light_completion(
                &mut stream,
                visible,
                DirectSkyMask::Uniform(false),
                false,
                false,
                [false; 6],
            );
            stream.accept_light_completion(completion);
        }
        stream.poll_deadline = Some(Instant::now());
        if stream.dispatch_mesh_jobs(view.position, 1) != 0 {
            let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_ne!(completion.key, visible);
            assert_eq!(completion.key, backlog);
            published += 1;
            stream.accept_mesh_completion(completion);
            stream.take_mesh_changes();
        }
        let lane = &stream.mesh_jobs.lanes[RESIDENT_MESH_LANE];
        assert!(
            lane.ready.len() + lane.deferred.len() <= storage_limit,
            "unchanged retry accumulated heap records"
        );
        assert_eq!(stream.mesh_jobs.pending[&visible].revision, revision);
    }
    assert!(
        published >= 3,
        "memory-blocked retries starved admissible ready work across rotations: {published}"
    );
}

/// Successful far retries must retire lane ownership while a lower-ranked ready backlog stays busy.
#[test]
fn repeated_far_retry_publications_keep_storage_bounded_by_live_work() {
    let (mut stream, view, visible, backlog) = deferred_mesh_stream();
    for _ in 0..MAX_PENDING_SCHEDULER_SCANS_PER_POLL {
        stream.poll_deadline = Some(Instant::now());
        assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
        let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(completion.key, visible);
        stream.accept_mesh_completion(completion);
        stream.take_mesh_changes();
        assert!(stream.mesh_jobs.pending.contains_key(&backlog));
        let lane = &stream.mesh_jobs.lanes[RESIDENT_MESH_LANE];
        assert!(
            lane.ready.len() + lane.deferred.len() <= stream.mesh_jobs.pending.len(),
            "published retry retained obsolete lane ownership"
        );
        stream.mark_dirty_exact(visible, Instant::now());
    }
}

/// An incomplete halo must not consume the dispatch opportunity of an independent ready mesh.
#[test]
fn partial_light_halo_does_not_delay_ready_geometry() {
    for retry in [false, true] {
        let (mut stream, view, visible, backlog) = deferred_mesh_stream();
        stream.mesh_jobs.retries.clear();
        if retry {
            stream.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                .ready
                .remove(&backlog);
            let revision = stream.mesh_jobs.pending[&backlog].revision;
            stream.mesh_jobs.retries.enqueue(backlog, revision);
        }
        let pending = stream.mesh_jobs.pending[&visible];
        let first = SubChunkKey::new(1, visible.x + 1, 1, 1);
        let second = SubChunkKey::new(1, visible.x - 1, 1, 1);
        for key in [first, second] {
            install_current_light(&mut stream, key, 0, 0, false);
            stream.mark_light_dirty_exact(key).unwrap();
        }
        install_current_light(&mut stream, first, 0, 0, false);
        let direct = stream.lighting.direct_sky[&first].clone();
        stream.finish_accepted_light_completion(first, 0, &direct, [false; 6], false);
        assert!(stream.mesh_light_halo(visible).is_none());
        assert!(stream.mesh_light_halo(backlog).is_some());
        assert_eq!(
            stream.mesh_jobs.pending[&visible].revision,
            pending.revision
        );
        assert_eq!(
            stream.mesh_jobs.pending[&visible].queued_at,
            pending.queued_at
        );
        stream.poll_deadline = Some(Instant::now());
        assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
        let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(completion.key, backlog);
    }
}

/// A completed face may invalidate another halo member before deferred geometry can become ready.
#[test]
fn light_completion_fanout_preserves_ready_geometry() {
    let (mut stream, view, visible, backlog) = deferred_mesh_stream();
    stream.mesh_jobs.retries.clear();
    let source = SubChunkKey::new(1, visible.x + 1, 1, 1);
    let neighbour = SubChunkKey::new(1, source.x, source.y - 1, source.z);
    install_current_light(&mut stream, source, 15, 0, false);
    install_current_light(&mut stream, neighbour, 0, 0, false);
    assert!(stream.mesh_light_halo(visible).is_some());
    let direct = stream.lighting.direct_sky[&source].clone();
    let changed_faces = LIGHT_NEIGHBOUR_OFFSETS.map(|offset| offset == [0, -1, 0]);
    stream.finish_accepted_light_completion(source, 0, &direct, changed_faces, false);
    assert!(stream.lighting.jobs.pending.contains_key(&neighbour));
    assert!(stream.mesh_light_halo(visible).is_none());
    assert!(stream.mesh_light_halo(backlog).is_some());
    stream.poll_deadline = Some(Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, backlog);
}
