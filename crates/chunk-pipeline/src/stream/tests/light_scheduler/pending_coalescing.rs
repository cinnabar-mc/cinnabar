use super::*;

#[test]
fn queued_light_changes_preserve_one_revision_age_and_ingress_entry() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    install_current_light(&mut stream, key, 0, 0, false);
    stream.mark_light_changed_sources([key]);
    let pending = stream.lighting.jobs.pending[&key];
    let generation = stream.lighting.block_generations[&key];
    for _ in 0..32 {
        stream.mark_light_changed_sources([key]);
    }
    assert!(stream.lighting.block_generations[&key] > generation);
    assert_eq!(
        stream.lighting.jobs.pending[&key].revision,
        pending.revision
    );
    assert_eq!(
        stream.lighting.jobs.pending[&key].queued_at,
        pending.queued_at
    );
    assert_eq!(stream.lighting.jobs.scan.len(), 1);
    assert_eq!(stream.lighting.revisions.entries.len(), 1);
    complete_one_light(&mut stream, [8.0; 3]);
    assert!(stream.light_is_current(key));
    assert_eq!(stream.stats.accepted_light_jobs, 1);
}

#[test]
fn queued_light_urgency_promotes_once_and_retains_dependency_wakeups() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    let dependency = SubChunkKey::new(1, 1, 0, 0);
    install_current_light(&mut stream, key, 0, 0, false);
    let revision = stream.mark_light_dirty_exact(key).unwrap();
    let queued_at = stream.lighting.jobs.pending[&key].queued_at;
    stream
        .lighting
        .waiters
        .entry(dependency)
        .or_default()
        .insert(key);
    stream.lighting.priority_wakeups.insert(key, revision);
    for urgent in [false, true, true, false, true] {
        assert_eq!(
            stream.mark_light_dirty_exact_with_priority(key, urgent),
            Some(revision)
        );
    }
    assert!(stream.lighting.jobs.pending[&key].urgent);
    assert_eq!(stream.lighting.jobs.pending[&key].queued_at, queued_at);
    assert_eq!(stream.lighting.jobs.scan.len(), 2);
    assert_eq!(stream.lighting.priority_wakeups.get(&key), Some(&revision));
    assert!(stream.lighting.waiters[&dependency].contains(&key));
}

#[test]
fn first_in_flight_change_invalidates_snapshot_and_later_changes_share_successor() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    install_current_light(&mut stream, key, 0, 0, false);
    let original = Arc::clone(stream.lighting.store.light(key).unwrap());
    let mut completion = synthetic_light_completion(
        &mut stream,
        key,
        DirectSkyMask::Uniform(false),
        false,
        false,
        [false; 6],
    );
    completion.identity.urgent = true;
    stream
        .lighting
        .jobs
        .in_flight
        .insert(key, completion.identity);
    stream.lighting.jobs.scan.clear();
    stream.mark_light_changed_sources([key]);
    let successor = stream.lighting.jobs.pending[&key];
    assert_ne!(successor.revision, completion.identity.revision);
    assert!(successor.urgent);
    for _ in 0..32 {
        stream.mark_light_changed_sources([key]);
    }
    assert_eq!(
        stream.lighting.jobs.pending[&key].revision,
        successor.revision
    );
    assert_eq!(stream.lighting.jobs.scan.len(), 1);
    stream.accept_light_completion(completion);
    assert_eq!(stream.stats.stale_light_jobs, 1);
    assert_eq!(
        stream.lighting.jobs.pending[&key].revision,
        successor.revision
    );
    complete_one_light(&mut stream, [8.0; 3]);
    assert!(stream.light_is_current(key));
    assert!(Arc::ptr_eq(
        stream.lighting.store.light(key).unwrap(),
        &original
    ));
    assert_eq!(stream.stats.noop_light_jobs, 1);
}

/// A pending seeded relight falls back to a full solve once a neighbour's relight changes the
/// light on their shared face.
#[test]
fn a_neighbour_relight_drops_pending_changed_cells_across_its_changed_face() {
    let mut stream = lit_stream(0);
    let west = SubChunkKey::new(0, 0, 0, 0);
    let east = SubChunkKey::new(0, 1, 0, 0);
    for key in [west, east] {
        stream
            .authority
            .commit_sub_chunk(key, uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
    }
    for (key, cell) in [
        (west, BlockPos::new(8, 8, 8)),
        (east, BlockPos::new(24, 8, 8)),
    ] {
        stream.mark_light_changed_sources_reaching(
            [key],
            true,
            NeighbourReach::of_cell(8, 8, 8),
            Some(&[cell]),
        );
    }
    assert!(stream.lighting.changed_cells.contains_key(&west));
    assert!(stream.lighting.changed_cells.contains_key(&east));
    stream
        .lighting
        .exclusion_waiters
        .entry(west)
        .or_default()
        .insert(east);
    let mut changed_faces = [false; 6];
    changed_faces[LIGHT_NEIGHBOUR_OFFSETS
        .iter()
        .position(|&offset| offset_sub_chunk_key(west, offset) == Some(east))
        .unwrap()] = true;
    let direct_sky = stream.lighting.direct_sky[&west].clone();

    stream.finish_accepted_light_completion(west, 1, &direct_sky, changed_faces, true);

    assert!(stream.lighting.jobs.pending.contains_key(&east));
    assert!(!stream.lighting.changed_cells.contains_key(&east));
}

/// Scheduling exclusions preserve an interior edit until every unchanged blocker finishes.
#[test]
fn unchanged_exclusion_completions_preserve_incremental_light_input() {
    for blocker_count in [1, 2] {
        let mut stream = lit_stream(1);
        let target = SubChunkKey::new(1, 1, 0, 0);
        let blockers = [SubChunkKey::new(1, 0, 0, 0), SubChunkKey::new(1, 1, 0, -1)];
        for key in std::iter::once(target).chain(blockers[..blocker_count].iter().copied()) {
            stream
                .authority
                .commit_sub_chunk(key, uniform_sub_chunk(2))
                .unwrap();
            install_current_light(&mut stream, key, 0, 0, false);
        }
        for &source in &blockers[..blocker_count] {
            stream.mark_light_dirty_exact(source).unwrap();
        }
        assert_eq!(
            stream.dispatch_light_jobs([8.0; 3], blocker_count),
            blocker_count
        );
        let mut completions = Vec::new();
        test_time::eventually("unchanged exclusion blockers", || {
            completions.extend(stream.lighting.rx.try_iter());
            completions.len() == blocker_count
        });

        let cell = BlockPos::new(24, 8, 8);
        let mutation = ChunkStore::prepare_sub_chunk_blocks(
            target,
            stream.authority.terrain().sub_chunk(target).as_deref(),
            &[BlockUpdate::new(8, 8, 8, 0, 1)],
            stream.classifier.air_network_id(),
        )
        .unwrap();
        assert!(stream.commit_block_mutations(vec![mutation]));
        let pending = stream.lighting.jobs.pending[&target];
        assert_eq!(stream.lighting.changed_cells[&target], [cell]);

        while !completions.is_empty() {
            // The worker has finished, but its identity still excludes the edited target.
            assert_eq!(
                stream.dispatch_light_jobs_reserving([24.0, 8.0, 8.0], 1, 1),
                0
            );
            assert!(
                stream
                    .lighting
                    .exclusion_waiters
                    .values()
                    .any(|waiters| waiters.contains(&target))
            );
            let completion = completions.pop().unwrap();
            let solved = completion.result.as_ref().unwrap();
            assert!(!solved.light_levels_changed);
            assert!(!solved.direct_sky_changed);
            assert_eq!(solved.changed_faces, [false; 6]);
            stream.accept_light_completion(completion);
            assert_eq!(
                stream.stats.noop_light_jobs,
                (blocker_count - completions.len()) as u64
            );
            assert_eq!(
                stream.lighting.jobs.pending[&target].revision,
                pending.revision
            );
            assert_eq!(
                stream.lighting.jobs.pending[&target].queued_at,
                pending.queued_at
            );
            assert_eq!(
                stream
                    .lighting
                    .changed_cells
                    .get(&target)
                    .map(Vec::as_slice),
                Some(&[cell][..])
            );
        }

        let job = stream.take_pending_light_job(target);
        assert_eq!(job.changed.as_deref(), Some(&[cell][..]));
        let solved = solve_prepared_light_job(job).unwrap();
        assert_eq!(
            solved.replacement.get(LightChannel::Block, 8, 8, 8),
            Some(15)
        );
        assert!(stream.lighting.waiters.is_empty());
        assert!(stream.lighting.exclusion_waiters.is_empty());
    }
}

/// A retained untrusted boundary still invalidates seeds when a scheduling edge also exists.
#[test]
fn untrusted_waiter_takes_precedence_over_an_unchanged_exclusion_wake() {
    let (mut stream, source, target, cell) = seeded_adjacent_light();
    let completion = synthetic_light_completion(
        &mut stream,
        source,
        DirectSkyMask::Uniform(false),
        false,
        false,
        [false; 6],
    );
    for waiters in [
        &mut stream.lighting.waiters,
        &mut stream.lighting.exclusion_waiters,
    ] {
        waiters.entry(source).or_default().insert(target);
    }
    assert_eq!(stream.lighting.changed_cells[&target], [cell]);

    stream.accept_light_completion(completion);

    assert_eq!(stream.stats.noop_light_jobs, 1);
    assert!(stream.take_pending_light_job(target).changed.is_none());
    assert!(stream.lighting.waiters.is_empty());
    assert!(stream.lighting.exclusion_waiters.is_empty());
}

/// Stale completions cannot certify an exclusion's boundary or bypass input validation.
#[test]
fn stale_exclusion_completion_keeps_the_seed_until_untrusted_input_validation() {
    let (mut stream, source, target, cell) = seeded_adjacent_light();
    let completion = synthetic_light_completion(
        &mut stream,
        source,
        DirectSkyMask::Uniform(false),
        false,
        false,
        [false; 6],
    );
    let pending = stream.lighting.jobs.pending[&target];
    assert_eq!(stream.dispatch_light_jobs([24.0, 8.0, 8.0], 1), 0);
    assert!(stream.lighting.exclusion_waiters[&source].contains(&target));
    stream.mark_light_dirty_exact(source).unwrap();

    stream.accept_light_completion(completion);

    assert_eq!(stream.stats.stale_light_jobs, 1);
    assert_eq!(stream.stats.noop_light_jobs, 0);
    assert_eq!(stream.lighting.changed_cells[&target], [cell]);
    assert_eq!(
        stream.lighting.jobs.pending[&target].revision,
        pending.revision
    );
    assert_eq!(
        stream.lighting.jobs.pending[&target].queued_at,
        pending.queued_at
    );
    assert!(stream.lighting.exclusion_waiters[&source].contains(&target));
    assert!(stream.take_pending_light_job(target).changed.is_none());
    assert!(stream.lighting.exclusion_waiters.is_empty());
}

/// Per-key removal, fatal shutdown and complete retirement release scheduling edges.
#[test]
fn exclusion_waiters_follow_lighting_lifecycle_cleanup() {
    for retirement in 0..4 {
        let (mut stream, source, target, _) = seeded_adjacent_light();
        stream
            .lighting
            .exclusion_waiters
            .entry(source)
            .or_default()
            .insert(target);
        match retirement {
            0 => stream.lighting.remove_key(source),
            1 => stream.lighting.remove_key(target),
            2 => stream.lighting.clear_after_fatal(),
            3 => stream.lighting.retire_all(),
            _ => unreachable!(),
        }
        assert!(stream.lighting.exclusion_waiters.is_empty());
    }
}

/// Builds two settled opaque sections and queues an interior-only incremental edit in the east.
fn seeded_adjacent_light() -> (WorldStream, SubChunkKey, SubChunkKey, BlockPos) {
    let mut stream = lit_stream(1);
    let source = SubChunkKey::new(1, 0, 0, 0);
    let target = SubChunkKey::new(1, 1, 0, 0);
    for key in [source, target] {
        stream
            .authority
            .commit_sub_chunk(key, uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
    }
    let cell = BlockPos::new(24, 8, 8);
    stream.mark_light_changed_sources_reaching(
        [target],
        true,
        NeighbourReach::of_cell(8, 8, 8),
        Some(&[cell]),
    );
    (stream, source, target, cell)
}
