//! Candidate scratch preserves stable visibility priority while reusing storage.

use super::*;

#[test]
fn warmed_candidate_preparation_reuses_storage_and_preserves_scan_order() {
    let requests: Vec<Candidate> = (0..512)
        .map(|index| (ChunkKey::new(1, index as i32, 0), index))
        .collect();
    let expected: Vec<_> = requests
        .iter()
        .copied()
        .filter(|&(_, index)| index % 7 != 0 && index % 3 == 0)
        .chain(
            requests
                .iter()
                .copied()
                .filter(|&(_, index)| index % 7 != 0 && index % 3 != 0),
        )
        .collect();
    let mut scratch = PreparationScratch::default();
    for _ in 0..3 {
        scratch.prepare_candidates(
            requests.iter().copied(),
            |(_, index)| index % 7 != 0,
            |(_, index)| index % 3 == 0,
        );
    }
    let mut viewed = 0;
    let (_, allocations) = test_alloc::measure(|| {
        scratch.prepare_candidates(
            requests.iter().copied(),
            |(_, index)| index % 7 != 0,
            |(_, index)| {
                viewed += 1;
                index % 3 == 0
            },
        );
    });
    assert_eq!(allocations, test_alloc::Allocations::default());
    assert_eq!(scratch.candidates, expected);
    assert_eq!(
        viewed,
        requests.iter().filter(|(_, index)| index % 7 != 0).count()
    );
    scratch.prepare_candidates(
        requests.iter().copied(),
        |(_, index)| index % 2 == 0,
        |(_, index)| index % 5 == 0,
    );
    let changed: Vec<_> = requests
        .iter()
        .copied()
        .filter(|&(_, index)| index % 2 == 0 && index % 5 == 0)
        .chain(
            requests
                .iter()
                .copied()
                .filter(|&(_, index)| index % 2 == 0 && index % 5 != 0),
        )
        .collect();
    assert_eq!(scratch.candidates, changed);
    scratch.prepare_candidates(requests.iter().copied(), |_| false, |_| unreachable!());
    assert!(scratch.candidates.is_empty());
}

/// Changing worlds must not repeat catalog-sized preparation for the same entity assets.
#[test]
fn world_change_reuses_spawner_catalog_preparation() {
    for variables in [16, 2048] {
        let initialize = (0..variables)
            .map(|index| format!("variable.fixture_{index} = 0;"))
            .collect::<String>();
        let (pool, _, _) = fixture_with_scripts(serde_json::json!({
            "initialize": [initialize],
        }));
        let source = actor_animation::ActorAnimationCatalog::new(Arc::clone(&pool.source));
        let session = (pool.session.0 + 1, pool.session.1 + 1);
        let (_, allocations) =
            test_alloc::measure(|| Pool::replacement(Some(pool), source, session));
        assert!(
            allocations.calls <= 64 && allocations.bytes <= 64 * 1024,
            "world change repeated preparation for {variables} variables: {allocations:?}"
        );
    }
}

/// Reusing a catalog must leave both the old namespace and its server pack behind.
#[test]
fn world_change_retires_display_lifetimes_and_pack_bindings() {
    let (mut previous, _, _) = fixture();
    let (other, _, _) = fixture_with_identifier(serde_json::json!({}), "1", "custom:other");
    previous
        .actors
        .set_pack(Some((Arc::clone(&other.source), vec![0])));
    let position = [1, 2, 3];
    let runtime = previous.actors.resolve(position, "custom:other").unwrap();
    previous.actors.advance(3);
    let old_session = previous.actors.session_id();
    let source = actor_animation::ActorAnimationCatalog::new(Arc::clone(&previous.source));
    let session = (previous.session.0 + 1, previous.session.1 + 1);
    let mut next = Pool::replacement(Some(previous), source, session);
    assert_ne!(next.actors.session_id(), old_session);
    assert!(next.actors.actor(runtime).is_none());
    assert!(next.actors.resolve(position, "custom:other").is_none());
    assert!(next.actors.resolve(position, "custom:display").is_some());
}

/// Replacing the asset allocation must prepare the new catalog instead of retaining old rigs.
#[test]
fn world_change_uses_replacement_entity_assets() {
    let (previous, _, _) = fixture();
    let (other, _, _) = fixture_with_identifier(serde_json::json!({}), "1", "custom:other");
    let source = actor_animation::ActorAnimationCatalog::new(Arc::clone(&other.source));
    let session = (previous.session.0 + 1, previous.session.1 + 1);
    let mut next = Pool::replacement(Some(previous), source, session);
    assert!(next.actors.resolve([0; 3], "custom:display").is_none());
    assert!(next.actors.resolve([0; 3], "custom:other").is_some());
}

/// Fresh and disconnected joins publish display state without repeating the worker's analysis.
#[test]
fn first_spawner_pool_reuses_join_catalog_preparation() {
    for variables in [16, 2048] {
        let initialize = (0..variables)
            .map(|index| format!("variable.fixture_{index} = 0;"))
            .collect::<String>();
        let (previous, _, _) = fixture_with_scripts(serde_json::json!({
            "initialize": [initialize],
        }));
        let catalog = actor_animation::ActorAnimationCatalog::new(Arc::clone(&previous.source));
        let session = (previous.session.0 + 1, previous.session.1);
        drop(previous);
        let (mut pool, allocations) =
            test_alloc::measure(|| Pool::replacement(None, catalog, session));
        assert!(
            allocations.calls <= 64 && allocations.bytes <= 64 * 1024,
            "join repeated preparation for {variables} variables: {allocations:?}"
        );
        assert!(pool.actors.resolve([0; 3], "custom:display").is_some());
    }
}
