use super::*;

/// Camera movement reorders distant notifications before lower-priority ordinary geometry.
#[test]
fn camera_move_reranks_far_dependency_notifications() {
    let mut stream = lit_stream(1);
    let side = world::SUB_CHUNK_SIDE as f32;
    let before = stream.scheduler_view([side / 2.0; 3]);
    stream
        .mesh_jobs
        .refresh
        .refresh(before, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    let old_best = SubChunkKey::new(1, 8, 0, 0);
    let new_best = SubChunkKey::new(1, 12, 0, 0);
    let ordinary = SubChunkKey::new(1, 10, 0, 0);
    for key in [old_best, new_best, ordinary] {
        queue_mesh(&mut stream, before, key, key == ordinary);
    }
    stream.mesh_jobs.scan.clear();
    for key in [old_best, new_best] {
        let direct = stream.lighting.direct_sky[&key].clone();
        stream.finish_accepted_light_completion(key, 0, &direct, [false; 6], false);
    }
    let after = [(17.0 + 0.5) * side, side / 2.0, side / 2.0];
    assert!(!scheduler::mesh_is_near(
        stream.scheduler_view(after),
        new_best
    ));

    assert_eq!(stream.dispatch_mesh_jobs(after, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, new_best);
    stream.accept_mesh_completion(completion);
    assert!(stream.mesh_jobs.pending.contains_key(&ordinary));
}

/// A second camera move during refresh eventually orders both generations for the newest view.
#[test]
fn far_notifications_survive_camera_moves_before_refresh_finishes() {
    let mut stream = lit_stream(1);
    let side = world::SUB_CHUNK_SIDE as f32;
    let before = stream.scheduler_view([side / 2.0; 3]);
    stream
        .mesh_jobs
        .refresh
        .refresh(before, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    let count = MAX_PENDING_SCHEDULER_SCANS_PER_POLL * 3;
    for offset in 0..count {
        let key = SubChunkKey::new(1, -64 - i32::try_from(offset).unwrap(), 0, 0);
        queue_mesh(&mut stream, before, key, true);
    }
    let old_best = SubChunkKey::new(1, 8, 0, 0);
    let new_best = SubChunkKey::new(1, 12, 0, 0);
    let ordinary = SubChunkKey::new(1, 10, 0, 0);
    for key in [old_best, new_best, ordinary] {
        queue_mesh(&mut stream, before, key, key == ordinary);
    }
    stream.mesh_jobs.scan.clear();
    let middle = stream.scheduler_view([2.5 * side, side / 2.0, side / 2.0]);
    stream.mesh_jobs.refresh.refresh(
        middle,
        &mut stream.mesh_jobs.lanes,
        Some(Instant::now()),
        |_, _| true,
    );
    assert_eq!(stream.mesh_jobs.lanes[RESIDENT_MESH_LANE].ready.len(), 1);
    let pending = stream.mesh_jobs.pending[&new_best];
    for _ in 0..32 {
        for key in [old_best, new_best] {
            let direct = stream.lighting.direct_sky[&key].clone();
            stream.finish_accepted_light_completion(key, 0, &direct, [false; 6], false);
        }
    }
    let after = stream.scheduler_view([17.5 * side, side / 2.0, side / 2.0]);
    assert!(!scheduler::mesh_is_near(after, new_best));
    let mut settled = false;
    for _ in 0..12 {
        // Refresh owns all ranks; this routes no new work and performs no geometry checks.
        if !stream
            .mesh_jobs
            .ingress(after, None, |_, _, _| (RESIDENT_MESH_LANE, false))
        {
            settled = true;
            break;
        }
        assert_eq!(stream.mesh_jobs.pending.len(), count + 3);
    }
    assert!(
        settled,
        "camera refresh left notified work in an old generation"
    );
    assert_eq!(
        stream.mesh_jobs.pending[&new_best].queued_at,
        pending.queued_at
    );
    assert_eq!(stream.mesh_jobs.pending[&new_best].since, pending.since);
    for expected in [new_best, ordinary, old_best] {
        assert_eq!(stream.dispatch_mesh_jobs(after.position, 1), 1);
        let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(completion.key, expected);
        stream.accept_mesh_completion(completion);
    }
    assert_eq!(stream.mesh_jobs.pending.len(), count);
}

/// Blocked notifications yield to ordinary geometry while a camera generation is still draining.
#[test]
fn blocked_far_notifications_allow_geometry_during_camera_refresh() {
    let mut stream = lit_stream(1);
    let before = stream.scheduler_view([8.0; 3]);
    stream
        .mesh_jobs
        .refresh
        .refresh(before, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    let blocked = SubChunkKey::new(1, 8, 0, 0);
    let ordinary = SubChunkKey::new(1, 10, 0, 0);
    queue_mesh(&mut stream, before, blocked, false);
    queue_mesh(&mut stream, before, ordinary, true);
    let count = MAX_PENDING_SCHEDULER_SCANS_PER_POLL * 3;
    for offset in 0..count {
        let key = SubChunkKey::new(1, -64 - i32::try_from(offset).unwrap(), 0, 0);
        queue_mesh(&mut stream, before, key, true);
    }
    stream.mesh_jobs.scan.clear();
    stream.mark_light_dirty_exact(blocked).unwrap();
    let neighbour = SubChunkKey::new(1, blocked.x, 1, 0);
    install_current_light(&mut stream, neighbour, 0, 0, false);
    let direct = stream.lighting.direct_sky[&neighbour].clone();
    stream.finish_accepted_light_completion(neighbour, 0, &direct, [false; 6], false);
    let camera = [40.0, 8.0, 8.0];
    let mut dispatched = false;
    for _ in 0..3 {
        stream.poll_deadline = Some(Instant::now());
        if stream.dispatch_mesh_jobs(camera, 1) == 1 {
            dispatched = true;
            break;
        }
    }
    assert!(dispatched, "blocked notification stopped ordinary geometry");
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, ordinary);
    stream.accept_mesh_completion(completion);
    assert!(stream.mesh_jobs.pending.contains_key(&blocked));
    assert!(stream.mesh_jobs.lanes[RESIDENT_MESH_LANE].ready.len() < count);
}

/// Notifications preserve ordinary retry cooldown and queued age, and clearing retires the owner.
#[test]
fn notified_mesh_retains_retry_history_until_queued_work_is_cleared() {
    let mut stream = lit_stream(1);
    let view = stream.scheduler_view([8.0; 3]);
    let key = SubChunkKey::new(1, 12, 0, 0);
    queue_mesh(&mut stream, view, key, false);
    let pending = stream.mesh_jobs.pending[&key];
    for _ in 0..4 {
        stream.mesh_jobs.retries.block(key, pending.revision);
    }
    for _ in 0..32 {
        let direct = stream.lighting.direct_sky[&key].clone();
        stream.finish_accepted_light_completion(key, 0, &direct, [false; 6], false);
    }
    stream.mesh_jobs.retries.begin_poll();
    let retry = stream.mesh_jobs.retries.pop().unwrap();
    assert_eq!(retry.key, key);
    assert_eq!(retry.revision, pending.revision);
    assert!(!stream.mesh_jobs.retries.is_due(&retry));
    stream.mesh_jobs.retries.requeue(retry);
    assert_eq!(stream.mesh_jobs.pending[&key].queued_at, pending.queued_at);
    assert_eq!(stream.mesh_jobs.pending[&key].since, pending.since);
    stream.mesh_jobs.clear_queued();
    assert!(stream.mesh_jobs.pending.is_empty());
    assert!(stream.mesh_jobs.lanes[RESIDENT_MESH_LANE].is_empty());
    assert!(stream.mesh_jobs.retries.pop().is_none());
    let direct = stream.lighting.direct_sky[&key].clone();
    stream.finish_accepted_light_completion(key, 0, &direct, [false; 6], false);
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 0);
}

/// Repeated notifications share one check across generations, then wake after their halo settles.
#[test]
fn previous_generation_notifications_coalesce_and_make_progress() {
    let mut stream = lit_stream(1);
    let before = stream.scheduler_view([8.0; 3]);
    stream
        .mesh_jobs
        .refresh
        .refresh(before, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    let visible = SubChunkKey::new(1, 12, 0, 0);
    let ordinary = SubChunkKey::new(1, 16, 0, 0);
    queue_mesh(&mut stream, before, visible, false);
    queue_mesh(&mut stream, before, ordinary, true);
    for offset in 0..MAX_PENDING_SCHEDULER_SCANS_PER_POLL * 3 {
        let key = SubChunkKey::new(1, -64 - i32::try_from(offset).unwrap(), 0, 0);
        queue_mesh(&mut stream, before, key, true);
    }
    stream.mesh_jobs.scan.clear();
    stream.mark_light_dirty_exact(visible).unwrap();
    let neighbour = SubChunkKey::new(1, visible.x, 1, 0);
    install_current_light(&mut stream, neighbour, 0, 0, false);
    let direct = stream.lighting.direct_sky[&neighbour].clone();
    stream.finish_accepted_light_completion(neighbour, 0, &direct, [false; 6], false);
    let after = stream.scheduler_view([40.0, 8.0, 8.0]);
    stream.mesh_jobs.refresh.refresh(
        after,
        &mut stream.mesh_jobs.lanes,
        Some(Instant::now()),
        |_, _| true,
    );
    for _ in 0..32 {
        stream.finish_accepted_light_completion(neighbour, 0, &direct, [false; 6], false);
    }
    meshing::MESH_READINESS_CHECKS.with(|checks| checks.set(0));
    stream.poll_deadline = Some(Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs(after.position, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, ordinary);
    stream.accept_mesh_completion(completion);
    assert_eq!(
        meshing::MESH_READINESS_CHECKS.with(std::cell::Cell::get),
        0,
        "a previous-generation notification was duplicated or checked by the retry rotation"
    );

    stream.poll_deadline = None;
    let mut settled = false;
    for _ in 0..8 {
        if !stream
            .mesh_jobs
            .ingress(after, None, |_, _, _| (RESIDENT_MESH_LANE, false))
        {
            settled = true;
            break;
        }
    }
    assert!(settled);
    assert_eq!(stream.dispatch_mesh_jobs(after.position, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_ne!(completion.key, visible);
    stream.accept_mesh_completion(completion);
    assert_eq!(meshing::MESH_READINESS_CHECKS.with(std::cell::Cell::get), 1);
    assert!(stream.mesh_jobs.pending.contains_key(&visible));

    install_current_light(&mut stream, visible, 0, 0, false);
    stream.finish_accepted_light_completion(neighbour, 0, &direct, [false; 6], false);
    assert_eq!(stream.dispatch_mesh_jobs(after.position, 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.key, visible);
    stream.accept_mesh_completion(completion);
}

/// Adds a current geometry revision to one lane without scheduler ingress or worker activity.
fn queue_mesh(stream: &mut WorldStream, view: SchedulerView, key: SubChunkKey, ready: bool) {
    stream
        .authority
        .commit_sub_chunk(key, super::uniform_sub_chunk(2))
        .unwrap();
    install_current_light(stream, key, 0, 0, false);
    let revision = stream.mark_dirty_exact(key, Instant::now());
    let candidate = PendingSchedulerCandidate::new(key, revision, view, false);
    let lane = &mut stream.mesh_jobs.lanes[RESIDENT_MESH_LANE];
    if ready {
        lane.ready.push(candidate);
    } else {
        lane.deferred.push(candidate);
    }
}
