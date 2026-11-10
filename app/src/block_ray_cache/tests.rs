use assets::block_shapes::PickContext;
use bevy::ecs::change_detection::Tick;
use client_interaction::movement::PhysicsCollisionRegistries;
use sim::{PaletteWorld, Vec3};

use super::{BlockRayCache, CachedPickWorld};

const EYE: Vec3 = Vec3::new(4.5, 3.62, 8.5);
/// Meets the south face of the stone at (4, 2, 6) 1.749 blocks away.
const TO_STONE: Vec3 = Vec3::new(0.0, -0.6, -1.0);
/// Meets the north face of the water at (4, 2, 10).
const TO_WATER: Vec3 = Vec3::new(0.0, -0.6, 1.0);
const UP: Vec3 = Vec3::new(0.0, 1.0, 0.0);

struct Fixture {
    stream: chunk_pipeline::WorldStream,
    collisions: PhysicsCollisionRegistries,
    id: Box<dyn Fn(&str) -> u32>,
}

/// One loaded column holding a stone row at z = 6 and a water source at (4, 2, 10).
fn fixture() -> Fixture {
    let breg = assets::pinned_block_registry_bytes();
    let protocol = assets::active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(breg, protocol).unwrap();
    let collisions = PhysicsCollisionRegistries::from_assets(
        breg,
        &records,
        include_bytes!("../../../crates/assets/data/block-physics-v2225.bin"),
        protocol,
    )
    .unwrap();
    let id = move |name: &str| {
        records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap()
            .sequential_id
    };
    let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 42,
        local_player_unique_id: 1,
        player_position: [4.5, 2.0, 8.5],
        world_spawn_position: [4, 1, 8],
        air_network_id: id("minecraft:air"),
        block_network_ids_are_hashes: false,
    });
    let mut biome_payload = vec![1, 2];
    biome_payload.extend(std::iter::repeat_n(
        0xff,
        protocol::vanilla_dimension_range(0)
            .unwrap()
            .sub_chunk_count
            - 1,
    ));
    biome_payload.push(0);
    stream
        .submit(
            1,
            protocol::WorldEvent::LevelChunk(protocol::LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: protocol::LevelChunkMode::LimitedRequests { highest: 0 },
                payload: biome_payload,
            }),
        )
        .unwrap();
    let mut blocks: Vec<_> = (4..=6)
        .map(|x| ([x, 2, 6], id("minecraft:stone")))
        .collect();
    blocks.push(([4, 2, 10], id("minecraft:water")));
    let mut fixture = Fixture {
        stream,
        collisions,
        id: Box::new(id),
    };
    fixture.set_blocks(2, &blocks);
    fixture
}

impl Fixture {
    /// Commits `blocks` at `sequence` and waits for the store to apply them.
    fn set_blocks(&mut self, sequence: u64, blocks: &[([i32; 3], u32)]) {
        self.stream
            .submit(
                sequence,
                protocol::WorldEvent::BlockUpdates(
                    blocks
                        .iter()
                        .map(|&(position, network_id)| protocol::BlockUpdateEvent {
                            dimension: 0,
                            position,
                            layer: 0,
                            network_id,
                        })
                        .collect(),
                ),
            )
            .unwrap();
        let stream = &mut self.stream;
        test_time::eventually("fixture blocks commit", || {
            stream.poll([4.5, 2.0, 8.5], 0);
            stream.committed_sequence() >= sequence
        });
    }

    fn ids(&self) -> client_world::ingestion::DecodeIds {
        self.stream.authority().decode_ids(0)
    }

    fn picks<'a>(
        &'a self,
        cache: &'a BlockRayCache,
        context: PickContext,
        tick: u32,
        ids: &'a client_world::ingestion::DecodeIds,
    ) -> CachedPickWorld<'a> {
        CachedPickWorld::new(
            cache,
            &self.stream,
            self.collisions.registry(self.stream.network_id_mode()),
            Tick::new(tick),
            context,
            ids,
        )
    }

    /// The query surface with no cache, built independently of [`CachedPickWorld::world`].
    fn uncached<'a>(
        &'a self,
        context: PickContext,
        ids: &'a client_world::ingestion::DecodeIds,
    ) -> PaletteWorld<'a> {
        PaletteWorld::new(
            self.stream.collision_store(),
            self.collisions.registry(self.stream.network_id_mode()),
            0,
            context,
        )
        .with_cell_overlays(ids)
    }
}

fn entries(cache: &BlockRayCache) -> usize {
    cache.0.lock().unwrap().len()
}

fn liquids() -> PickContext {
    PickContext {
        liquids: true,
        ..PickContext::default()
    }
}

#[test]
fn cached_rays_equal_the_uncached_queries_and_repeat_without_recomputing() {
    let fixture = fixture();
    let ids = fixture.ids();
    let cache = BlockRayCache::default();
    let ordinary = PickContext::default();
    let picks = fixture.picks(&cache, ordinary, 1, &ids);
    let world = fixture.uncached(ordinary, &ids);

    let hit = picks.ray(EYE, TO_STONE, 5.7);
    assert_eq!(hit, world.block_interaction_ray_current(EYE, TO_STONE, 5.7));
    assert_eq!(hit.as_ref().unwrap().as_ref().unwrap().block_pos, [4, 2, 6]);
    let miss = picks.ray(EYE, UP, 5.7);
    assert_eq!(miss, world.block_interaction_ray_current(EYE, UP, 5.7));
    assert_eq!(miss, Ok(None));
    let support = picks.miss_support(EYE, UP);
    assert_eq!(support, world.block_use_miss_support_current(EYE, UP));
    let observed = picks.observed_ray(EYE, TO_STONE, 5.7);
    assert_eq!(
        observed,
        fixture
            .uncached(ordinary, &ids)
            .observe(|world| world.block_interaction_ray_current(EYE, TO_STONE, 5.7))
    );
    assert_eq!(entries(&cache), 4);

    // Every repeat, including one through a new view of the same inputs, reuses its entry.
    let fresh_ids = fixture.ids();
    let again = fixture.picks(&cache, ordinary, 1, &fresh_ids);
    assert_eq!(again.ray(EYE, TO_STONE, 5.7), hit);
    assert_eq!(again.ray(EYE, UP, 5.7), miss);
    assert_eq!(again.miss_support(EYE, UP), support);
    assert_eq!(again.observed_ray(EYE, TO_STONE, 5.7), observed);
    assert_eq!(entries(&cache), 4);
}

#[test]
fn each_reach_prefix_and_liquid_context_keeps_its_own_result() {
    let fixture = fixture();
    let ids = fixture.ids();
    let cache = BlockRayCache::default();
    for context in [PickContext::default(), liquids()] {
        let picks = fixture.picks(&cache, context, 1, &ids);
        let world = fixture.uncached(context, &ids);
        for reach in [1.7, 1.749, 1.75, 1.8, 3.0, 5.7] {
            for direction in [TO_STONE, TO_WATER] {
                assert_eq!(
                    picks.ray(EYE, direction, reach),
                    world.block_interaction_ray_current(EYE, direction, reach),
                    "{context:?} {reach} {direction:?}"
                );
                for prefix in [0.0, 1.0, 1.7, reach, reach + 1.0, f64::INFINITY] {
                    assert_eq!(
                        picks.ray_prefix(EYE, direction, reach, prefix),
                        world.block_interaction_ray_prefix_current(EYE, direction, reach, prefix),
                        "{context:?} {reach} {direction:?} {prefix}"
                    );
                }
            }
        }
    }
    // The reach boundary separates a miss from the hit, and only a liquid pick meets water.
    let ordinary = fixture.picks(&cache, PickContext::default(), 1, &ids);
    assert_eq!(ordinary.ray(EYE, TO_STONE, 1.7), Ok(None));
    assert!(ordinary.ray(EYE, TO_STONE, 1.8).unwrap().is_some());
    assert_eq!(ordinary.ray(EYE, TO_WATER, 3.0), Ok(None));
    let liquid = fixture.picks(&cache, liquids(), 1, &ids);
    let water = liquid.ray(EYE, TO_WATER, 3.0).unwrap().unwrap();
    assert_eq!(water.block_pos, [4, 2, 10]);
}

#[test]
fn a_world_or_registry_change_retires_cached_results() {
    let mut fixture = fixture();
    let cache = BlockRayCache::default();
    let ordinary = PickContext::default();
    let before = {
        let ids = fixture.ids();
        let picks = fixture.picks(&cache, ordinary, 1, &ids);
        let before = picks.ray(EYE, TO_STONE, 5.7);
        assert_eq!(before, picks.ray(EYE, TO_STONE, 5.7));
        assert_eq!(entries(&cache), 1);
        // An in-place registry edit carries a new change tick.
        assert_eq!(picks.ray(EYE, TO_STONE, 5.7), before);
        let edited = fixture.picks(&cache, ordinary, 2, &ids);
        assert_eq!(edited.ray(EYE, TO_STONE, 5.7), before);
        assert_eq!(entries(&cache), 2);
        before
    };

    let air = (fixture.id)("minecraft:air");
    fixture.set_blocks(3, &[([4, 2, 6], air)]);
    let ids = fixture.ids();
    let picks = fixture.picks(&cache, ordinary, 1, &ids);
    let after = picks.ray(EYE, TO_STONE, 5.7);
    assert_eq!(
        after,
        fixture
            .uncached(ordinary, &ids)
            .block_interaction_ray_current(EYE, TO_STONE, 5.7)
    );
    assert_ne!(after, before);
    assert_eq!(entries(&cache), 3);
}
