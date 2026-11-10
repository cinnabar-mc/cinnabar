use super::*;
use assets::{
    BlockFlags, BlockOverlay, BlockVisual, ContributorRole, LightProperties, Material,
    NO_ANIMATION, NO_MODEL_TEMPLATE, RuntimeAssets, TextureArray, TextureMip, TextureRef,
    VisualKind, VisualSupport,
};
use particles::EmptyWorld;
use protocol_ids::LevelEvent;

const BLOCK_HASH: u32 = 0xfabc_def0;
const OTHER_HASH: u32 = 0x1234_5678;
const BLOCK_COLOR: [u8; 4] = [240, 16, 16, 255];
const OTHER_COLOR: [u8; 4] = [16, 16, 240, 255];

/// Supplies two distinct block tiles without requiring a local asset carrier.
fn stream(hashes: bool) -> WorldStream {
    let visual = BlockVisual {
        native_material: assets::native_material::NativeMaterialFacts::Unresolved,
        faces: [0; 6],
        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        kind: VisualKind::Cube,
        support: VisualSupport::Exact,
        contributor_role: ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
        destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
    };
    let overlay = BlockOverlay {
        visuals: vec![
            visual,
            BlockVisual {
                faces: [1; 6],
                ..visual
            },
        ],
        light_properties: vec![LightProperties::OPAQUE_DARK; 2],
        hashes: vec![Some(BLOCK_HASH), Some(OTHER_HASH)],
        materials: (0..2)
            .map(|layer| Material {
                texture: TextureRef::new(1, layer).unwrap(),
                ..Material::unvaried()
            })
            .collect(),
        textures: vec![TextureArray {
            layers: 2,
            mips: (0..assets::MIP_COUNT)
                .map(|level| {
                    let size = assets::TILE_SIZE >> level;
                    TextureMip {
                        size,
                        rgba8: [BLOCK_COLOR, OTHER_COLOR]
                            .into_iter()
                            .flat_map(|color| color.repeat((size * size) as usize))
                            .collect(),
                    }
                })
                .collect(),
        }],
        ..BlockOverlay::default()
    };
    WorldStream::new_with_assets(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [8.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: if hashes {
                protocol_ids::HASHED_AIR_NETWORK_ID
            } else {
                protocol_ids::SEQUENTIAL_AIR_NETWORK_ID
            },
            block_network_ids_are_hashes: hashes,
        },
        Arc::new(
            RuntimeAssets::diagnostic()
                .with_block_overlay(1, &overlay)
                .unwrap(),
        ),
        [8.0; 3],
        None,
    )
}

/// Routes one packet through the app and advances its synthetic terrain emitter.
fn route(stream: &WorldStream, event_id: i32, data: i32) -> ParticleSystem {
    let registry = sim::CollisionRegistry::default();
    let world = StreamParticleWorld::new(stream, &registry, None);
    let routing = Routing {
        world: &world,
        stream,
        mode: stream.network_id_mode(),
        icons: None,
    };
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(
        br#"{"particle_effect":{"description":{"identifier":"minecraft:block_destruct",
        "basic_render_parameters":{"material":"particles_alpha","texture":"atlas.terrain"}},
        "components":{"minecraft:emitter_lifetime_once":{},
        "minecraft:emitter_rate_instant":{"num_particles":"v.emitter_particles_count"},
        "minecraft:particle_lifetime_expression":{"max_lifetime":1},
        "minecraft:particle_appearance_billboard":{"size":[0.05,0.05]}}}}"#
    ));
    route_level_event(
        &mut system,
        &routing,
        &mut client_audio::audio::EchoLedger::default(),
        &protocol::LevelParticleEvent {
            event_id,
            position: [8.5; 3],
            data,
        },
        0.0,
        0,
    );
    system.tick(0.001, &EmptyWorld);
    system
}

/// Publishes a cell whose material differs from the event's material.
fn load_other_block(stream: &mut WorldStream, network_id: u32) {
    let mut payload = vec![1, 2];
    payload.extend(std::iter::repeat_n(
        0xff,
        protocol::vanilla_dimension_range(0)
            .unwrap()
            .sub_chunk_count
            - 1,
    ));
    payload.push(0);
    stream
        .submit(
            1,
            protocol::WorldEvent::LevelChunk(protocol::LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: protocol::LevelChunkMode::LimitedRequests { highest: 0 },
                payload,
            }),
        )
        .unwrap();
    stream
        .submit(
            2,
            protocol::WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
                dimension: 0,
                position: [8; 3],
                layer: 0,
                network_id,
            }]),
        )
        .unwrap();
    test_time::eventually("crack fixture cell publication", || {
        stream.poll([8.5; 3], 0);
        stream.committed_sequence() == 2
    });
    assert_eq!(
        stream
            .collision_store()
            .sub_chunk(world::SubChunkKey::new(0, 0, 0, 0))
            .unwrap()
            .runtime_id(0, 8, 8, 8),
        Some(network_id)
    );
}

#[test]
fn crack_packets_use_the_event_block_before_and_after_terrain_arrives() {
    for hashes in [false, true] {
        let mut stream = stream(hashes);
        let runtime_id = if hashes { BLOCK_HASH } else { 1 };
        let event_id = LevelEvent::ParticlesCrackBlockEast.wire_value();
        for loaded in [false, true] {
            if loaded {
                load_other_block(&mut stream, if hashes { OTHER_HASH } else { 2 });
            }
            let system = route(&stream, event_id, runtime_id as i32);
            assert_eq!(system.live_particles(), 1);
            assert!(
                system
                    .atlas()
                    .pixels()
                    .as_chunks::<4>()
                    .0
                    .contains(&BLOCK_COLOR)
            );
            assert!(
                !system
                    .atlas()
                    .pixels()
                    .as_chunks::<4>()
                    .0
                    .contains(&OTHER_COLOR)
            );
            if !hashes {
                assert_eq!(
                    route(
                        &stream,
                        LevelEvent::ParticlesCrackBlock.wire_value(),
                        (5 << 24) | runtime_id as i32
                    )
                    .live_particles(),
                    1
                );
            }
        }
    }
}

#[test]
fn unknown_hash_cracks_do_not_borrow_the_loaded_blocks_texture() {
    let mut stream = stream(true);
    load_other_block(&mut stream, OTHER_HASH);
    for event_id in LevelEvent::ParticlesCrackBlockDown.wire_value()
        ..=LevelEvent::ParticlesCrackBlockEast.wire_value()
    {
        assert_eq!(
            route(&stream, event_id, 0xdead_beef_u32 as i32).live_particles(),
            0
        );
    }
    assert_eq!(
        route(
            &stream,
            LevelEvent::ParticlesCrackBlock.wire_value(),
            (5 << 24) | 0x00ad_beef,
        )
        .live_particles(),
        0
    );
}
