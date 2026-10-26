//! Particle fluid reads keep primary-layer semantics without allocating physics samples.

use super::*;
use sim::SurfaceResponse;
use world::{BlockUpdate, ChunkStore, SubChunkKey};

/// A loaded section with original air, water and lava movement facts.
fn fixture() -> (ChunkStore, CollisionRegistry) {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, 0, 0);
    store.apply_request_mode_air(key).unwrap();
    store.mark_sub_chunk_loaded(key).unwrap();
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    for (id, flags) in [(1, BlockPhysicsFlags::WATER), (2, BlockPhysicsFlags::LAVA)] {
        registry
            .register_physics(id, [], 0.6, 1.0, 1.0, 1.0, flags, SurfaceResponse::None)
            .unwrap();
    }
    (store, registry)
}

/// Replaces one test cell's storage layer through the normal palette update path.
fn set(store: &mut ChunkStore, x: u8, layer: u32, runtime_id: u32) {
    store
        .update_block(
            SubChunkKey::new(0, 0, 0, 0),
            BlockUpdate::new(x, 0, 0, layer, runtime_id),
            0,
        )
        .unwrap();
}

#[test]
fn recurring_particle_fluid_reads_allocate_nothing() {
    let (mut store, registry) = fixture();
    set(&mut store, 1, 0, 1);
    set(&mut store, 2, 0, 2);
    set(&mut store, 3, 1, 1);
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());
    let expected = [Fluid::None, Fluid::Water, Fluid::Lava, Fluid::None];
    let ((), allocations) = test_alloc::count(|| {
        for _ in 0..128 {
            for (x, expected) in expected.into_iter().enumerate() {
                assert_eq!(fluid_at(&world, [x as i32, 0, 0]), expected);
            }
        }
    });
    assert_eq!(
        allocations, 0,
        "fluid checks must not allocate sample storage"
    );
}

#[test]
fn particle_fluid_reads_discard_results_when_any_layer_is_unknown() {
    let (mut store, registry) = fixture();
    set(&mut store, 0, 0, 1);
    set(&mut store, 0, 2, 99);
    set(&mut store, 1, 0, 99);
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());
    for position in [[0, 0, 0], [1, 0, 0], [16, 0, 0], [0, 16, 0]] {
        assert_eq!(fluid_at(&world, position), Fluid::None, "{position:?}");
    }
}

#[test]
fn particle_fluid_reads_observe_material_replacements() {
    let (mut store, registry) = fixture();
    for (runtime_id, expected) in [(1, Fluid::Water), (2, Fluid::Lava), (0, Fluid::None)] {
        set(&mut store, 0, 0, runtime_id);
        let world = PaletteWorld::new(&store, &registry, 0, Default::default());
        assert_eq!(fluid_at(&world, [0, 0, 0]), expected);
    }
}
