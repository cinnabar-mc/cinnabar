use super::*;
use assets::RuntimeAssets;

#[test]
fn fog_samples_outside_nether_height_keep_the_dimension_biome() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("assets/bedrock-target.json")).unwrap())
            .unwrap();
    let path = root.join(target["artifacts"]["world_assets"].as_str().unwrap());
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!(
            "SKIP fog_samples_outside_nether_height_keep_the_dimension_biome: missing {}",
            path.display()
        );
        return;
    };
    let assets = RuntimeAssets::decode(&bytes).unwrap();
    let stream = WorldStream::new(protocol::WorldBootstrap {
        dimension: 1,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.5, 129.0, 0.5],
        world_spawn_position: [0, 64, 0],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let bindings = FogBindings::for_profiles(
        &assets.biome_assets().rules,
        &[assets::BiomeVisualProfile {
            biome_identifier: "minecraft:hell".into(),
            fog_identifier: "test:fog".into(),
            atmosphere_identifier: "test:atmosphere".into(),
            lighting_identifier: "test:lighting".into(),
            sky_rgb8: None,
        }],
        &[assets::FogProfile {
            identifier: "test:fog".into(),
            distances: Box::new([]),
        }],
    );
    let mut cached = CameraBiomes::default();
    for height in [129.0, -1.0] {
        let position = [0.5, height, 0.5];
        let biomes = lattice_biomes(&stream, position);
        assert_eq!(
            fog_biome_samples(&biomes, stream.current_dimension(), &bindings),
            [Some(FogProfileIndex(0)); render_model::PRECIPITATION_SAMPLE_OFFSETS.len()]
        );
        assert_eq!(
            cached.sample(&stream, position),
            (stream.camera_biome_id(position), biomes)
        );
    }
}

#[test]
fn weather_and_fog_use_one_floored_native_lattice() {
    let origin = [-1.25, 63.875, 2.5];
    let positions = sample_positions(origin);
    for (actual, offset) in positions.iter().zip(PRECIPITATION_SAMPLE_OFFSETS) {
        assert_eq!(
            *actual,
            std::array::from_fn(|axis| origin[axis].floor() + offset[axis] as f32)
        );
    }
    assert_eq!(positions.len(), PRECIPITATION_SAMPLE_OFFSETS.len());
}

#[test]
fn missing_cells_never_get_renormalized_or_assigned_fallback_precipitation() {
    assert_eq!(count_precipitation_samples([None, None]), None);
    assert_eq!(count_precipitation_samples([Some(false), None]), Some(0));
    assert_eq!(
        count_precipitation_samples([Some(true), None, Some(false)]),
        Some(1)
    );
}

/// Bindings that route `minecraft:hell` (raw id 8) to the fog profile `fog`, out of `test:a` and
/// `test:b`.
fn hell_bindings(fog: &str) -> FogBindings {
    FogBindings::for_profiles(
        &[assets::BiomeRule {
            id: 8,
            name: "minecraft:hell".into(),
            flags: 0,
            grass: assets::TintSource::direct(0),
            foliage: assets::TintSource::direct(0),
            dry_foliage: assets::TintSource::direct(0),
            water: assets::TintSource::direct(0),
            temperature_bits: 0,
            downfall_bits: 0,
        }],
        &[assets::BiomeVisualProfile {
            biome_identifier: "minecraft:hell".into(),
            fog_identifier: fog.into(),
            atmosphere_identifier: "test:atmosphere".into(),
            lighting_identifier: "test:lighting".into(),
            sky_rgb8: None,
        }],
        &[
            assets::FogProfile {
                identifier: "test:a".into(),
                distances: Box::new([]),
            },
            assets::FogProfile {
                identifier: "test:b".into(),
                distances: Box::new([]),
            },
        ],
    )
}

#[test]
fn cached_lattice_fog_matches_fresh_samples_across_lattice_and_binding_changes() {
    let stream = WorldStream::new(protocol::WorldBootstrap {
        dimension: 1,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.5, 129.0, 0.5],
        world_spawn_position: [0, 64, 0],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let fresh = |biomes: &LatticeBiomes, bindings: &FogBindings| {
        (
            fog_biome_samples(biomes, stream.current_dimension(), bindings),
            fog_precipitation_count(&stream, biomes),
        )
    };
    let first = hell_bindings("test:a");
    let second = hell_bindings("test:b");
    assert_ne!(first.revision(), second.revision());
    let hell = [Some(8); render_model::PRECIPITATION_SAMPLE_OFFSETS.len()];
    let unknown = [None; render_model::PRECIPITATION_SAMPLE_OFFSETS.len()];
    let mut cache = FogLatticeCache::default();
    for (biomes, bindings) in [
        (&hell, &first),
        (&hell, &first),
        (&unknown, &first),
        (&hell, &second),
        (&hell, &first),
    ] {
        assert_eq!(
            cache.sample(&stream, biomes, bindings),
            fresh(biomes, bindings)
        );
    }
    assert_ne!(fresh(&hell, &first).0, fresh(&hell, &second).0);
}

#[test]
fn cached_lattice_fog_follows_the_biome_registry_and_the_dimension() {
    let registry = |downfall| assets::ResolvedBiomeTints {
        skipped_definitions: 0,
        records: Box::new([]),
        biomes: vec![
            None,
            Some(assets::SessionBiome {
                identifier: "minecraft:hell".into(),
                temperature: 2.0,
                downfall,
            }),
        ]
        .into(),
        swamp_grass_palette: Box::new([]),
        raw_id_to_dense: (0..9).map(|raw| u32::from(raw == 8)).collect(),
    };
    let dry = registry(0.0);
    let wet = registry(0.5);
    let dry_id = meshing::ChunkBiomeTintIdentity::new(1, 1);
    let wet_id = meshing::ChunkBiomeTintIdentity::new(1, 2);
    let bindings = hell_bindings("test:a");
    let hell = [Some(8); render_model::PRECIPITATION_SAMPLE_OFFSETS.len()];
    let unknown = [None; render_model::PRECIPITATION_SAMPLE_OFFSETS.len()];
    let fresh = |biomes: &LatticeBiomes, dimension, registry| {
        (
            fog_biome_samples(biomes, dimension, &bindings),
            precipitation_count(registry, biomes),
        )
    };
    // The same raw id with another downfall changes the count; another dimension changes the
    // fallback profile of unknown samples.
    assert_ne!(fresh(&hell, 1, &dry).1, fresh(&hell, 1, &wet).1);
    assert_ne!(fresh(&unknown, 1, &wet).0, fresh(&unknown, 0, &wet).0);

    let mut cache = FogLatticeCache::default();
    for (biomes, dimension, id, registry) in [
        (&hell, 1, dry_id, &dry),
        (&hell, 1, wet_id, &wet),
        (&hell, 1, dry_id, &dry),
        (&unknown, 1, wet_id, &wet),
        (&unknown, 0, wet_id, &wet),
        (&unknown, 1, wet_id, &wet),
    ] {
        assert_eq!(
            cache.sample_registry(biomes, dimension, id, registry, &bindings),
            fresh(biomes, dimension, registry)
        );
    }
}

#[test]
fn cached_lattice_fog_through_streams_follows_each_stream_and_its_dimension() {
    let stream = |dimension| {
        WorldStream::new(protocol::WorldBootstrap {
            dimension,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.5, 70.0, 0.5],
            world_spawn_position: [0, 64, 0],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        })
    };
    let (overworld, nether, other_nether) = (stream(0), stream(1), stream(1));
    assert_ne!(
        nether.biome_tint_identity(),
        other_nether.biome_tint_identity()
    );
    let bindings = hell_bindings("test:a");
    let unknown = [None; render_model::PRECIPITATION_SAMPLE_OFFSETS.len()];
    let fresh = |stream: &WorldStream| {
        (
            fog_biome_samples(&unknown, stream.current_dimension(), &bindings),
            fog_precipitation_count(stream, &unknown),
        )
    };
    // Unknown samples take their dimension's fallback profile, which only the nether binds.
    assert_ne!(fresh(&overworld).0, fresh(&nether).0);

    let mut cache = FogLatticeCache::default();
    for stream in [&nether, &overworld, &nether, &other_nether, &overworld] {
        assert_eq!(cache.sample(stream, &unknown, &bindings), fresh(stream));
    }
}
