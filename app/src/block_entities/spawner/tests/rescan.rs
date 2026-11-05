use super::*;

/// Commits identifier changes through the stream and waits for their terrain snapshots.
fn update_identifiers(
    world: &mut bevy::prelude::World,
    positions: &[[i32; 3]],
    identifier: &str,
    sequence: &mut u64,
) {
    let mut client = world.resource_mut::<ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let dimension = stream.current_dimension();
    for &position in positions {
        stream
            .submit(
                *sequence,
                protocol::WorldEvent::BlockEntityUpdate(protocol::BlockEntityUpdateEvent {
                    dimension,
                    position,
                    nbt: entity_nbt("MobSpawner", identifier, position),
                }),
            )
            .unwrap();
        *sequence += 1;
    }
    test_time::eventually("spawner identifier updates", || {
        stream.poll([1024.0, 66.0, 1024.0], 0);
        positions.iter().all(|&[x, y, z]| {
            stream
                .collision_store()
                .block_entity(BlockEntityKey::new(dimension, x, y, z))
                .and_then(|nbt| nbt.parse())
                .is_some_and(|root| root.string("EntityIdentifier") == Some(identifier))
        })
    });
}

#[test]
fn regression_distant_spawner_scans_are_budgeted_and_near_edits_are_immediate() {
    let mut world = system_fixture(serde_json::json!({}), "1");
    let near = [1024, 64, 1024];
    let side = SUB_CHUNK_SIDE as i32;
    let limit = columns::MAX_COLUMN_RESCANS_PER_FRAME;
    let far: Vec<_> = (-2..=3)
        .flat_map(|x| (-2..=3).map(move |z| (x, z)))
        .filter(|&(x, z)| !columns::is_near_column([0, 0], x, z))
        .map(|(x, z)| [near[0] + x * side, near[1], near[2] + z * side])
        .filter(|&position| {
            display_center(position).distance_squared(Vec3::new(1024.0, 66.0, 1024.0))
                <= DISPLAY_SCAN_RADIUS * DISPLAY_SCAN_RADIUS
        })
        .take(limit * 3)
        .collect();
    assert_eq!(far.len(), limit * 3);
    let mut sequence = 2;
    {
        let mut client = world.resource_mut::<ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        for &position in &far {
            admit_spawners(stream, position, &[(position, "custom:display")], sequence);
            sequence += 1;
        }
    }
    for admitted in (limit..=far.len()).step_by(limit) {
        prepare_displays(&mut world, simulation_clock::TICK_DURATION);
        assert_eq!(
            published(&world),
            (admitted + 1) * 2,
            "only the distant rescan budget and the near display may appear this frame"
        );
    }
    let mut changed = far.clone();
    changed.push(near);
    update_identifiers(&mut world, &changed, "custom:missing", &mut sequence);
    for refreshed in (limit..=far.len()).step_by(limit) {
        prepare_displays(&mut world, simulation_clock::TICK_DURATION);
        let pool = world
            .resource::<SpawnerDisplayPublisher>()
            .0
            .as_ref()
            .unwrap();
        assert!(
            !pool.spins.contains_key(&near),
            "near edits must retire the old display even when the distant budget is spent"
        );
        assert_eq!(
            published(&world),
            (far.len() - refreshed) * 2,
            "deferred distant columns retain their display until their budgeted rescan"
        );
    }
}

/// The settled terrain revision and listed requests of the last column pass.
fn listing(world: &bevy::prelude::World) -> (Option<u64>, Vec<(ChunkKey, usize)>) {
    let pool = world
        .resource::<SpawnerDisplayPublisher>()
        .0
        .as_ref()
        .unwrap();
    let listing = pool.listing.as_ref().unwrap();
    (
        listing.settled.as_ref().map(|inputs| inputs.window.terrain),
        listing.requests.clone(),
    )
}

#[test]
fn regression_settled_listing_is_reused_only_while_the_terrain_is_unchanged() {
    let mut world = system_fixture(serde_json::json!({}), "1");
    prepare_displays(&mut world, simulation_clock::TICK_DURATION);
    let (settled, requests) = listing(&world);
    assert!(
        settled.is_some(),
        "an unchanged neighbourhood settles in one pass"
    );
    assert_eq!(requests.len(), 1);
    assert_eq!(published(&world), 2);

    prepare_displays(&mut world, simulation_clock::TICK_DURATION);
    assert_eq!(listing(&world), (settled, requests.clone()));
    assert_eq!(published(&world), 2);
    // A fresh pass over the same terrain lists exactly what the reused listing held.
    world
        .resource_mut::<SpawnerDisplayPublisher>()
        .0
        .as_mut()
        .unwrap()
        .listing = None;
    prepare_displays(&mut world, simulation_clock::TICK_DURATION);
    assert_eq!(listing(&world).1, requests);
    assert_eq!(published(&world), 2);

    let mut sequence = 2;
    update_identifiers(
        &mut world,
        &[[1024, 64, 1024]],
        "custom:missing",
        &mut sequence,
    );
    prepare_displays(&mut world, simulation_clock::TICK_DURATION);
    assert_ne!(
        listing(&world).0,
        settled,
        "a block-entity edit retires the settled terrain"
    );
    assert_eq!(
        published(&world),
        0,
        "the edited spawner's display retires on the next frame"
    );
}
