use super::*;

/// Installs a solved emitter section whose boundary light uses packed storage.
fn packed_stream() -> (WorldStream, SubChunkKey) {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    stream
        .authority
        .update_block(key, BlockUpdate::new(8, 8, 8, 0, 1), 0)
        .unwrap();
    stream.resident.insert(key);
    stream.mark_changed(key, Instant::now());
    complete_one_light(&mut stream, [8.0; 3]);
    (stream, key)
}

/// Runs the real solver against the immutable prior still held by acceptance.
fn unchanged_completion(stream: &mut WorldStream, key: SubChunkKey) -> LightCompletion {
    stream.mark_light_dirty_exact(key).unwrap();
    let job = stream.take_pending_light_job(key);
    LightCompletion {
        key,
        identity: job.identity,
        result: solve_prepared_light_job(job),
        queue_wait: Duration::ZERO,
        duration: Duration::ZERO,
    }
}

#[test]
fn equal_packed_completion_skips_owner_boundary_work() {
    let (mut stream, key) = packed_stream();
    let completion = unchanged_completion(&mut stream, key);
    let result = completion.result.as_ref().unwrap();
    assert!(!result.replacement.channel(LightChannel::Block).is_uniform());
    assert!(
        !result
            .replacement
            .channel(LightChannel::Block)
            .shares_packed_bytes_with(
                stream
                    .lighting
                    .store
                    .light(key)
                    .unwrap()
                    .channel(LightChannel::Block)
            )
    );
    assert!(!result.light_levels_changed);
    assert!(!result.direct_sky_changed);
    let before = stream.stats();

    let (_, work) =
        lighting::equality::work::measure(|| stream.accept_light_completion(completion));

    assert_eq!(
        stream.stats().accepted_light_jobs,
        before.accepted_light_jobs + 1
    );
    assert_eq!(stream.stats().noop_light_jobs, before.noop_light_jobs + 1);
    assert_eq!(
        work, [0; 4],
        "equal results need no owner boundary comparison"
    );
}

#[test]
fn equal_packed_completion_skips_worker_boundary_work() {
    let (mut stream, key) = packed_stream();

    let (completion, work) =
        lighting::equality::work::measure(|| unchanged_completion(&mut stream, key));

    let result = completion.result.unwrap();
    assert!(!result.light_levels_changed);
    assert!(!result.direct_sky_changed);
    assert_eq!(result.changed_faces, [false; 6]);
    assert_eq!(
        work, [0; 4],
        "complete equality needs no worker boundary comparison"
    );
}

/// Finalizes a chosen output using the production worker's captured target.
fn prepare_output(
    stream: &mut WorldStream,
    key: SubChunkKey,
    replacement: SubChunkLight,
    direct: DirectSkyMask,
) -> LightCompletion {
    stream.mark_light_dirty_exact(key).unwrap();
    let job = stream.take_pending_light_job(key);
    LightCompletion {
        key,
        identity: job.identity,
        result: Ok(lighting::types::finish_solved_light_job(
            lighting::types::light_batch_target(&job),
            replacement.with_generation(job.identity.revision),
            Arc::new(direct),
            false,
        )),
        queue_wait: Duration::ZERO,
        duration: Duration::ZERO,
    }
}

/// Makes provenance at one boundary cell without changing any other cell.
fn direct_bit([x, y, z]: [u8; 3]) -> DirectSkyMask {
    let mut words = Box::new([0_u64; 64]);
    let index = light_local_index(x, y, z);
    words[index / 64] |= 1 << (index % 64);
    DirectSkyMask::Packed(words)
}

#[test]
fn independently_owned_packed_channels_and_provenance_reuse_equality() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    install_current_light(&mut stream, key, 7, 7, false);
    let mut previous = stream.lighting.store.light(key).unwrap().as_ref().clone();
    for channel in [LightChannel::Block, LightChannel::Sky] {
        previous.set(channel, 8, 8, 8, 8).unwrap();
    }
    let mut replacement = previous.clone();
    for channel in [LightChannel::Block, LightChannel::Sky] {
        replacement.set(channel, 8, 8, 8, 9).unwrap();
        replacement.set(channel, 8, 8, 8, 8).unwrap();
        assert!(
            !previous
                .channel(channel)
                .shares_packed_bytes_with(replacement.channel(channel))
        );
    }
    stream.lighting.store.insert_known_air(key, previous);
    stream.lighting.direct_sky.get_mut(&key).unwrap().mask = Arc::new(direct_bit([0, 5, 7]));
    let (completion, worker) = lighting::equality::work::measure(|| {
        prepare_output(&mut stream, key, replacement, direct_bit([0, 5, 7]))
    });
    assert_eq!(worker, [0; 4]);
    assert!(completion.result.as_ref().unwrap().equal_prior.is_some());
    let before = stream.stats();

    let (_, owner) =
        lighting::equality::work::measure(|| stream.accept_light_completion(completion));

    assert_eq!(owner, [0; 4]);
    assert_eq!(stream.stats().noop_light_jobs, before.noop_light_jobs + 1);
}

#[test]
fn equal_uniform_outputs_skip_both_boundary_predicates() {
    for (block, sky, direct) in [(0, 0, false), (0, 15, true), (7, 7, false)] {
        let mut stream = lit_stream(1);
        let key = SubChunkKey::new(1, 0, 0, 0);
        install_current_light(&mut stream, key, block, sky, direct);
        let (completion, worker) = lighting::equality::work::measure(|| {
            prepare_output(
                &mut stream,
                key,
                SubChunkLight::uniform(block, sky, 0).unwrap(),
                DirectSkyMask::Uniform(direct),
            )
        });
        assert_eq!(worker, [0; 4]);
        assert_eq!(
            completion.result.as_ref().unwrap().changed_faces,
            [false; 6]
        );
        let before = stream.stats();

        let (_, owner) =
            lighting::equality::work::measure(|| stream.accept_light_completion(completion));

        assert_eq!(owner, [0; 4]);
        assert_eq!(stream.stats().noop_light_jobs, before.noop_light_jobs + 1);
        assert!(stream.light_is_current(key));
    }
}

#[test]
fn missing_or_invalid_prior_provenance_keeps_original_work_and_policy() {
    for invalid in [false, true] {
        let (mut stream, key) = packed_stream();
        let neighbour = offset_sub_chunk_key(key, [-1, 0, 0]).unwrap();
        install_current_light(&mut stream, neighbour, 15, 15, false);
        if invalid {
            stream
                .lighting
                .direct_sky
                .get_mut(&key)
                .unwrap()
                .light_revision += 1;
        } else {
            stream.lighting.direct_sky.remove(&key);
        }
        let replacement = stream.lighting.store.light(key).unwrap().as_ref().clone();
        let (completion, worker) = lighting::equality::work::measure(|| {
            prepare_output(&mut stream, key, replacement, DirectSkyMask::Uniform(false))
        });
        let solved = completion.result.as_ref().unwrap();
        assert!(solved.equal_prior.is_none());
        assert!(!solved.light_levels_changed);
        assert!(solved.direct_sky_changed);
        assert_eq!(worker[0], LIGHT_NEIGHBOUR_OFFSETS.len());
        assert!(worker[1] > 0);
        stream
            .lighting
            .waiters
            .entry(key)
            .or_default()
            .insert(neighbour);
        let before = stream.stats();

        let (_, owner) =
            lighting::equality::work::measure(|| stream.accept_light_completion(completion));

        assert_eq!(owner[2], 1);
        assert_eq!(owner[3] == 0, invalid);
        assert_eq!(
            stream.lighting.jobs.pending.contains_key(&neighbour),
            invalid
        );
        assert_eq!(
            stream.stats().provenance_only_light_jobs,
            before.provenance_only_light_jobs + 1
        );
        assert!(stream.light_is_current(key));
    }
}

#[test]
fn same_generation_mask_replacement_falls_back_for_gains_losses_and_equal_values() {
    for (captured, current) in [(false, true), (true, false), (false, false)] {
        let (mut stream, key) = packed_stream();
        let neighbour = offset_sub_chunk_key(key, [-1, 0, 0]).unwrap();
        install_current_light(&mut stream, neighbour, 15, 15, false);
        let mask = |set| {
            if set {
                direct_bit([0, 5, 7])
            } else {
                DirectSkyMask::Uniform(false)
            }
        };
        stream.lighting.direct_sky.get_mut(&key).unwrap().mask = Arc::new(mask(captured));
        let replacement = stream.lighting.store.light(key).unwrap().as_ref().clone();
        let completion = prepare_output(&mut stream, key, replacement, mask(captured));
        assert!(completion.result.as_ref().unwrap().equal_prior.is_some());
        let direct = stream.lighting.direct_sky.get_mut(&key).unwrap();
        let revision = direct.light_revision;
        direct.mask = Arc::new(mask(current));
        stream
            .lighting
            .waiters
            .entry(key)
            .or_default()
            .insert(neighbour);
        let before = stream.stats();

        let (_, owner) =
            lighting::equality::work::measure(|| stream.accept_light_completion(completion));

        assert_eq!(owner[2], 1);
        assert!(owner[3] > 0);
        assert_eq!(
            stream.lighting.jobs.pending.contains_key(&neighbour),
            !captured && current
        );
        assert_eq!(stream.stats().noop_light_jobs, before.noop_light_jobs + 1);
        assert_eq!(stream.lighting.direct_sky[&key].light_revision, revision);
        assert_eq!(
            stream.lighting.direct_sky[&key].mask.as_ref(),
            &mask(current)
        );
    }
}

#[test]
fn same_generation_light_replacement_falls_back_for_gains_losses_and_equal_values() {
    for delta in [-1_i16, 0, 1] {
        let (mut stream, key) = packed_stream();
        let neighbour = offset_sub_chunk_key(key, [-1, 0, 0]).unwrap();
        install_current_light(&mut stream, neighbour, 15, 15, false);
        let previous = stream.lighting.store.light(key).unwrap().as_ref().clone();
        let completion = prepare_output(
            &mut stream,
            key,
            previous.clone(),
            DirectSkyMask::Uniform(false),
        );
        let mut current = previous.clone();
        let value = i16::from(current.get(LightChannel::Block, 0, 5, 7).unwrap()) + delta;
        current
            .set(LightChannel::Block, 0, 5, 7, u8::try_from(value).unwrap())
            .unwrap();
        stream.lighting.store.insert_resident(key, current);
        stream
            .lighting
            .waiters
            .entry(key)
            .or_default()
            .insert(neighbour);
        let before = stream.stats();

        let (_, owner) =
            lighting::equality::work::measure(|| stream.accept_light_completion(completion));

        assert_eq!(owner[2], 1);
        assert!(owner[3] > 0);
        assert_eq!(
            stream.lighting.jobs.pending.contains_key(&neighbour),
            delta > 0
        );
        assert_eq!(stream.stats().noop_light_jobs, before.noop_light_jobs + 1);
        assert_eq!(
            stream.lighting.store.light(key).unwrap().generation(),
            previous.generation()
        );
    }
}

#[test]
fn changed_or_removed_provenance_revision_rejects_equal_noop() {
    for remove in [false, true] {
        let (mut stream, key) = packed_stream();
        let completion = unchanged_completion(&mut stream, key);
        if remove {
            stream.lighting.direct_sky.remove(&key);
        } else {
            stream
                .lighting
                .direct_sky
                .get_mut(&key)
                .unwrap()
                .light_revision += 1;
        }
        let before = stream.stats();

        let (_, owner) =
            lighting::equality::work::measure(|| stream.accept_light_completion(completion));

        assert_eq!(owner[2], 1);
        assert_eq!(stream.stats().stale_light_jobs, before.stale_light_jobs + 1);
        assert_eq!(
            stream.stats().accepted_light_jobs,
            before.accepted_light_jobs
        );
    }
}

#[test]
fn edited_or_evicted_equal_completion_is_rejected_before_boundary_work() {
    for evict in [false, true] {
        let (mut stream, key) = packed_stream();
        let completion = unchanged_completion(&mut stream, key);
        if evict {
            stream.evict_column(key.chunk());
        } else {
            stream
                .authority
                .update_block(key, BlockUpdate::new(8, 8, 8, 0, 2), 0)
                .unwrap();
            stream.mark_changed(key, Instant::now());
        }
        let before = stream.stats();

        let (_, work) =
            lighting::equality::work::measure(|| stream.accept_light_completion(completion));

        assert_eq!(work, [0; 4]);
        assert_eq!(stream.stats().stale_light_jobs, before.stale_light_jobs + 1);
        assert_eq!(
            stream.stats().accepted_light_jobs,
            before.accepted_light_jobs
        );
    }
}

#[test]
fn equality_identity_does_not_retain_retired_snapshots() {
    let (mut stream, key) = packed_stream();
    let light = Arc::downgrade(stream.lighting.store.light(key).unwrap());
    let direct = Arc::downgrade(&stream.lighting.direct_sky[&key].mask);
    let completion = unchanged_completion(&mut stream, key);
    let prior = completion
        .result
        .as_ref()
        .unwrap()
        .equal_prior
        .as_ref()
        .unwrap();
    assert!(prior.matches(
        stream.lighting.store.light(key),
        stream.lighting.direct_sky.get(&key)
    ));

    drop(stream);

    assert!(light.upgrade().is_none());
    assert!(direct.upgrade().is_none());
    assert!(!prior.matches(None, None));
}

#[test]
fn changed_outputs_keep_original_worker_and_owner_boundary_work() {
    for case in 0..4 {
        let (mut stream, key) = packed_stream();
        if case == 3 {
            stream.lighting.direct_sky.get_mut(&key).unwrap().mask =
                Arc::new(direct_bit([0, 5, 7]));
        }
        let previous = stream.lighting.store.light(key).unwrap().clone();
        let direct_before = stream.lighting.direct_sky[&key].mask.clone();
        let mut replacement = previous.as_ref().clone();
        if case < 2 {
            let level = replacement.get(LightChannel::Block, 0, 5, 7).unwrap();
            replacement
                .set(
                    LightChannel::Block,
                    0,
                    5,
                    7,
                    if case == 0 { level + 1 } else { level - 1 },
                )
                .unwrap();
        }
        let direct = if case == 2 {
            direct_bit([0, 5, 7])
        } else {
            DirectSkyMask::Uniform(false)
        };
        let (expected_changed, worker_before) = lighting::equality::work::measure(|| {
            LIGHT_NEIGHBOUR_OFFSETS.map(|offset| {
                light_face_changed(
                    Some(&previous),
                    &replacement,
                    Some(&direct_before),
                    &direct,
                    offset,
                )
            })
        });
        let (_, owner_before) = lighting::equality::work::measure(|| {
            stream.monotonic_light_faces(key, &replacement, &direct)
        });
        let (completion, worker_after) = lighting::equality::work::measure(|| {
            prepare_output(&mut stream, key, replacement, direct)
        });
        let solved = completion.result.as_ref().unwrap();
        assert!(solved.equal_prior.is_none());
        assert_eq!(solved.changed_faces, expected_changed);
        assert_eq!(worker_after, worker_before);

        let (_, owner_after) =
            lighting::equality::work::measure(|| stream.accept_light_completion(completion));

        assert_eq!(owner_after, owner_before);
    }
}
