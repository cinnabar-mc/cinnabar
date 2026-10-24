use std::cell::Cell;

use super::*;

/// One biome and one medium sample of the stream at `at`, counting real samples in `taken`.
struct Camera {
    at: [f32; 3],
    biome: TerrainSampleCache<[u32; 3], Option<u32>>,
    medium: TerrainSampleCache<[u32; 3], ::meshing::CameraMedium>,
    taken: Cell<usize>,
}

impl Camera {
    fn new(at: [f32; 3]) -> Self {
        Self {
            at,
            biome: TerrainSampleCache::default(),
            medium: TerrainSampleCache::default(),
            taken: Cell::new(0),
        }
    }

    /// Samples through both caches and checks them against sampling directly.
    fn sample(&mut self, stream: &WorldStream) -> (Option<u32>, ::meshing::CameraMedium) {
        let at = self.at;
        let taken = &self.taken;
        let biome = self
            .biome
            .sample(stream, at.map(f32::to_bits), |stream, _| {
                taken.set(taken.get() + 1);
                stream.camera_biome_id(at)
            });
        let medium = self
            .medium
            .sample(stream, at.map(f32::to_bits), |stream, _| {
                taken.set(taken.get() + 1);
                stream.camera_medium(at)
            });
        assert_eq!(biome, stream.camera_biome_id(at), "cached biome");
        assert_eq!(medium, stream.camera_medium(at), "cached medium");
        (biome, medium)
    }

    /// Whether the last `sample` resampled instead of reusing both values.
    fn resampled(&self, before: usize) -> bool {
        self.taken.get() > before
    }
}

#[test]
fn camera_samples_are_reused_until_their_terrain_assets_or_dimension_change() {
    let mut stream = WorldStream::new_with_assets(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(camera_medium_assets()),
        [0.0, 4.5, 0.0],
        None,
    );
    let key = SubChunkKey::new(0, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, uniform_sub_chunk(0))
        .unwrap();
    stream
        .authority
        .update_block(key, BlockUpdate::new(4, 4, 4, 0, 1), 0)
        .unwrap();
    stream.authority.commit_biome_column(
        key.chunk(),
        DecodedBiomeColumn::decode(0, 1, &[1, 84], &RAW_BIOMES),
    );
    let mut camera = Camera::new([4.5, 4.5, 4.5]);
    assert_eq!(
        camera.sample(&stream),
        (Some(42), ::meshing::CameraMedium::Water)
    );
    let taken = camera.taken.get();
    camera.sample(&stream);
    assert!(!camera.resampled(taken), "an unchanged frame reuses both");

    // Rewriting the same block changes nothing, so the samples stay.
    stream
        .authority
        .update_block(key, BlockUpdate::new(4, 4, 4, 0, 1), 0)
        .unwrap();
    camera.sample(&stream);
    assert!(!camera.resampled(taken));

    stream
        .authority
        .update_block(key, BlockUpdate::new(4, 4, 4, 0, 0), 0)
        .unwrap();
    let taken = camera.taken.get();
    assert_eq!(camera.sample(&stream).1, ::meshing::CameraMedium::Air);
    assert!(camera.resampled(taken), "a block edit at the camera");

    stream.authority.commit_biome_column(
        key.chunk(),
        DecodedBiomeColumn::decode(0, 1, &[1, 86], &RAW_BIOMES),
    );
    let taken = camera.taken.get();
    assert_eq!(camera.sample(&stream).0, Some(43));
    assert!(camera.resampled(taken), "a biome edit at the camera");

    let taken = camera.taken.get();
    camera.at = [5.5, 4.5, 4.5];
    camera.sample(&stream);
    assert!(camera.resampled(taken), "a moved camera");

    stream.reload_resource_assets(Arc::new(camera_medium_assets()));
    let taken = camera.taken.get();
    camera.sample(&stream);
    assert!(camera.resampled(taken), "replaced block assets");

    stream
        .submit(
            1,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0; 3],
                ..Default::default()
            }),
        )
        .unwrap();
    test_time::eventually("dimension change", || {
        stream.poll([0.0; 3], 0);
        stream.current_dimension() == 1
    });
    let taken = camera.taken.get();
    assert_eq!(camera.sample(&stream), (None, ::meshing::CameraMedium::Air));
    assert!(camera.resampled(taken), "a dimension change");
}
