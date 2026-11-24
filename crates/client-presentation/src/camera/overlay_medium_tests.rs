//! Embedded-eye classification using live palette and physics adapters.

use super::*;
use sim::{CollisionRegistry, SurfaceResponse};
use world::{BlockUpdate, ChunkStore, SubChunkKey};

/// Supplies a loaded cell with full scaffold collision and optional waterlogging.
fn fixture(primary: u32, water_height: Option<f64>) -> (ChunkStore, CollisionRegistry) {
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    let unit = Aabb::new(SimVec3::ZERO, SimVec3::ONE);
    for (id, flags) in [
        (1, BlockPhysicsFlags::SCAFFOLDING),
        (2, BlockPhysicsFlags::default()),
    ] {
        registry
            .register_physics(
                id,
                [unit],
                sim::DEFAULT_SURFACE_FRICTION,
                1.0,
                1.0,
                0.0,
                flags,
                SurfaceResponse::None,
            )
            .unwrap();
    }
    registry
        .register_physics(
            3,
            [],
            sim::DEFAULT_SURFACE_FRICTION,
            1.0,
            1.0,
            water_height.unwrap_or(1.0),
            BlockPhysicsFlags::WATER,
            SurfaceResponse::None,
        )
        .unwrap();
    let mut store = ChunkStore::new();
    load_cell(&mut store, primary, water_height.is_some());
    (store, registry)
}

/// Loads the fixture's sub-chunk into `store` with `primary` at the origin, under water when
/// `water` is set.
fn load_cell(store: &mut ChunkStore, primary: u32, water: bool) {
    let key = SubChunkKey::new(0, 0, 0, 0);
    store.apply_request_mode_air(key).unwrap();
    store.mark_sub_chunk_loaded(key).unwrap();
    store
        .update_block(key, BlockUpdate::new(0, 0, 0, 0, primary), 0)
        .unwrap();
    if water {
        store
            .update_block(key, BlockUpdate::new(0, 0, 0, 1, 3), 0)
            .unwrap();
    }
}

#[test]
fn scaffolding_keeps_the_embedded_view_clear_without_changing_water_or_stone() {
    for (primary, water_height, expected) in [
        (1, None, HeadMedium::Air),
        (1, Some(1.0), HeadMedium::Water),
        (1, Some(0.25), HeadMedium::Air),
        (2, None, HeadMedium::Solid),
        (0, None, HeadMedium::Air),
    ] {
        let (store, registry) = fixture(primary, water_height);
        let world = sim::PaletteWorld::new(&store, &registry, 0, Default::default());
        assert_eq!(
            probe_head_medium(&world, Vec3::splat(0.5)),
            expected,
            "primary={primary} water_height={water_height:?}"
        );
    }
}

#[test]
fn cached_eye_probe_follows_the_eye_the_terrain_and_registry_edits() {
    let (mut store, mut registry) = fixture(2, None);
    let fresh = |store: &ChunkStore, registry: &CollisionRegistry, eye: Vec3| {
        probe_head_medium(
            &sim::PaletteWorld::new(store, registry, 0, Default::default()),
            eye,
        )
    };
    let inside = Vec3::splat(0.5);
    let above = Vec3::new(0.5, 1.5, 0.5);
    let mut cache = HeadMediumCache::default();

    assert_eq!(
        cache.probe(&store, &registry, false, 0, inside),
        HeadMedium::Solid
    );
    assert_eq!(
        cache.probe(&store, &registry, false, 0, above),
        fresh(&store, &registry, above)
    );
    assert_eq!(
        cache.probe(&store, &registry, false, 0, inside),
        HeadMedium::Solid
    );

    // A block change under a still eye is seen through the store's content revision.
    store
        .update_block(
            SubChunkKey::new(0, 0, 0, 0),
            BlockUpdate::new(0, 0, 0, 0, 3),
            0,
        )
        .unwrap();
    assert_eq!(
        cache.probe(&store, &registry, false, 0, inside),
        HeadMedium::Water
    );
    assert_eq!(fresh(&store, &registry, inside), HeadMedium::Water);

    // Editing the registry in place keeps its identity, so only the caller's flag reveals it.
    assert!(registry.remove_runtime_id(3));
    registry
        .register_physics(
            3,
            [Aabb::new(SimVec3::ZERO, SimVec3::ONE)],
            sim::DEFAULT_SURFACE_FRICTION,
            1.0,
            1.0,
            0.0,
            BlockPhysicsFlags::default(),
            SurfaceResponse::None,
        )
        .unwrap();
    assert_eq!(
        cache.probe(&store, &registry, false, 0, inside),
        HeadMedium::Water,
        "an unflagged registry edit reuses the last probe"
    );
    assert_eq!(
        cache.probe(&store, &registry, true, 0, inside),
        HeadMedium::Solid
    );
    assert_eq!(fresh(&store, &registry, inside), HeadMedium::Solid);
}

#[test]
fn cached_eye_probe_matches_a_fresh_probe_across_liquid_surfaces_residency_and_dimensions() {
    let fresh = |store: &ChunkStore, registry: &CollisionRegistry, dimension, eye: Vec3| {
        probe_head_medium(
            &sim::PaletteWorld::new(store, registry, dimension, Default::default()),
            eye,
        )
    };
    let below = Vec3::new(0.5, 0.25, 0.5);
    let above = Vec3::new(0.5, 0.75, 0.5);
    let (mut store, registry) = fixture(1, Some(0.5));
    let mut cache = HeadMediumCache::default();
    let check = |cache: &mut HeadMediumCache, store: &ChunkStore, dimension, eye| {
        let expected = fresh(store, &registry, dimension, eye);
        assert_eq!(
            cache.probe(store, &registry, false, dimension, eye),
            expected
        );
        expected
    };

    // Both sides of a partial water surface inside one block.
    assert_eq!(check(&mut cache, &store, 0, below), HeadMedium::Water);
    assert_eq!(check(&mut cache, &store, 0, above), HeadMedium::Air);
    assert_eq!(check(&mut cache, &store, 0, below), HeadMedium::Water);

    // Another dimension at the same eye.
    check(&mut cache, &store, 1, below);
    check(&mut cache, &store, 0, below);

    // Unloading the column under a still eye, then loading it again.
    store.evict_chunk(SubChunkKey::new(0, 0, 0, 0).chunk());
    assert_eq!(check(&mut cache, &store, 0, below), HeadMedium::Air);
    load_cell(&mut store, 1, true);
    assert_eq!(check(&mut cache, &store, 0, below), HeadMedium::Water);

    // A different store with the same eye and registry.
    let (stone, _) = fixture(2, None);
    assert_eq!(check(&mut cache, &stone, 0, below), HeadMedium::Solid);
    assert_eq!(check(&mut cache, &store, 0, below), HeadMedium::Water);
}
