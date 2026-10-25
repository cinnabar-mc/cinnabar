#![expect(
    clippy::print_stderr,
    reason = "the benchmark reports biome record sizes on stderr"
)]

use std::{hint::black_box, sync::Arc};

use assets::{
    BlockFlags, BlockOverlay, BlockVisual, ContributorRole, LightProperties, Material, ModelQuad,
    ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE, NetworkIdMode, RuntimeAssets, TextureRef,
    VisualKind, VisualSupport,
};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use meshing::{
    BIOME_NEIGHBOUR_SLOT_COUNT, BlockClassifier, CellOverlay, ChunkMesh, Face, MeshLightSample,
    MeshOptions, PackedBiomeRecord, biome_neighbour_index, mesh_sub_chunk_in_neighbourhood,
    mesh_sub_chunk_in_neighbourhood_with_lighting, mesh_sub_chunk_in_neighbourhood_with_options,
};
use world::{
    BiomeStorage, DecodedBiomeColumn, MeshNeighbourhood, RawBiomeIds, RawBlockIds, SubChunk,
};

#[cfg(feature = "benchmark-mimalloc")]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

const AIR: u32 = 0;
const MODE: NetworkIdMode = NetworkIdMode::Sequential;
const OFFSETS: [[i8; 3]; 6] = [
    [-1, 0, 0],
    [1, 0, 0],
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
];

fn zig_zag_i32(value: i32) -> Vec<u8> {
    let mut value = ((value as u32) << 1) ^ ((value >> 31) as u32);
    let mut encoded = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        encoded.push(byte);
        if value == 0 {
            return encoded;
        }
    }
}

// Synthetic opaque cubes use the diagnostic texture, not diagnostic block semantics.
// This deliberately measures no model or liquid workloads.
fn terrain_assets() -> RuntimeAssets {
    terrain_assets_with_hashes(Vec::new())
}

/// Network hashes of the two terrain states in hashed-id fixtures.
const HASHED_TERRAIN: [u32; 2] = [0x8000_1111, 0x8000_2222];

/// Terrain states registered among as many hashed states as a vanilla registry carries, so
/// hash lookups search a realistic table.
fn hashed_terrain_assets() -> RuntimeAssets {
    let mut state = 0x9e37_79b9_u32;
    let mut hashes = (0..15_000)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            Some(state | 1)
        })
        .collect::<Vec<_>>();
    hashes[..2].copy_from_slice(&HASHED_TERRAIN.map(Some));
    terrain_assets_with_hashes(hashes)
}

/// Terrain cube states at sequential ids 1 and 2; with `hashes`, one state per hash.
fn terrain_assets_with_hashes(hashes: Vec<Option<u32>>) -> RuntimeAssets {
    let base = RuntimeAssets::diagnostic();
    let mut texture = base.texture_array().clone();
    texture.layers = 1;
    let visual = |material| BlockVisual {
        native_material: assets::native_material::NativeMaterialFacts::Unresolved,
        faces: [material; 6],
        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        kind: VisualKind::Cube,
        support: VisualSupport::VanillaFallback,
        contributor_role: ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
        destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
    };
    let material = Material {
        texture: TextureRef::new(1, 0).unwrap(),
        flags: 0,
        ..Material::unvaried()
    };
    let states = hashes.len().max(2);
    let mut visuals = vec![visual(0), visual(1)];
    visuals.resize(states, visual(0));
    base.with_block_overlay(
        1,
        &BlockOverlay {
            visuals,
            light_properties: vec![LightProperties::OPAQUE_DARK; states],
            materials: vec![material; 2],
            textures: vec![texture],
            hashes,
            ..Default::default()
        },
    )
    .unwrap()
}

fn uniform_chunk(id: u32) -> SubChunk {
    let mut payload = vec![9, 1, 0, 1];
    payload.extend(zig_zag_i32(id as i32));
    SubChunk::decode(&payload, &RawBlockIds { air: AIR })
}

fn mixed_id(x: u8, y: u8, z: u8) -> u32 {
    let height = 4 + (u32::from(x) * 3 + u32::from(z) * 5) % 8;
    if u32::from(y) >= height {
        AIR
    } else if y < 3 {
        1
    } else {
        2
    }
}

/// Scattered blocks in open air, as in sections above the surface or in large caves.
fn sparse_id(x: u8, y: u8, z: u8) -> u32 {
    match (u32::from(x) * 7 + u32::from(y) * 3 + u32::from(z) * 5) % 23 {
        0 => 1,
        1 => 2,
        _ => AIR,
    }
}

fn mixed_chunk() -> SubChunk {
    chunk_of(mixed_id)
}

fn chunk_of(id_at: fn(u8, u8, u8) -> u32) -> SubChunk {
    chunk_with_palette(id_at, [AIR, 1, 2])
}

/// `id_at` picks a palette index; `palette` maps it to network values.
fn chunk_with_palette(id_at: fn(u8, u8, u8) -> u32, palette: [u32; 3]) -> SubChunk {
    // Bedrock v9, one network storage, two bits/index, x-z-y ordering.
    let mut payload = vec![9, 1, 0, 5];
    let mut words = [0_u32; 256];
    for x in 0..16_u8 {
        for z in 0..16_u8 {
            for y in 0..16_u8 {
                let linear = (usize::from(x) << 8) | (usize::from(z) << 4) | usize::from(y);
                words[linear / 16] |= id_at(x, y, z) << ((linear % 16) * 2);
            }
        }
    }
    for word in words {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend(zig_zag_i32(3));
    for id in palette {
        payload.extend(zig_zag_i32(id as i32));
    }
    let chunk = SubChunk::decode(&payload, &RawBlockIds { air: AIR });
    for x in 0..16_u8 {
        for z in 0..16_u8 {
            for y in 0..16_u8 {
                assert_eq!(
                    chunk.runtime_id(0, x, y, z),
                    Some(palette[id_at(x, y, z) as usize])
                );
            }
        }
    }
    chunk
}

fn assert_geometry(mesh: &ChunkMesh, chunk: &SubChunk) {
    // Compare exposed unit-face area to the greedy output independently of quad splitting.
    let faces = [
        Face::NegativeX,
        Face::PositiveX,
        Face::NegativeY,
        Face::PositiveY,
        Face::NegativeZ,
        Face::PositiveZ,
    ];
    for (face, offset) in faces.into_iter().zip(OFFSETS) {
        let mut expected = 0_usize;
        for x in 0..16_u8 {
            for y in 0..16_u8 {
                for z in 0..16_u8 {
                    if chunk.runtime_id(0, x, y, z) == Some(AIR) {
                        continue;
                    }
                    let adjacent = [
                        i32::from(x) + i32::from(offset[0]),
                        i32::from(y) + i32::from(offset[1]),
                        i32::from(z) + i32::from(offset[2]),
                    ];
                    if adjacent.iter().any(|&v| !(0..16).contains(&v))
                        || chunk.runtime_id(
                            0,
                            adjacent[0] as u8,
                            adjacent[1] as u8,
                            adjacent[2] as u8,
                        ) == Some(AIR)
                    {
                        expected += 1;
                    }
                }
            }
        }
        let actual: usize = mesh
            .cube_quads()
            .iter()
            .filter(|quad| quad.face() == face)
            .map(|quad| usize::from(quad.width()) * usize::from(quad.height()))
            .sum();
        assert_eq!(actual, expected, "{face:?}");
    }
    assert_eq!(mesh.cube_quads().len(), mesh.cube_lighting().len());
    assert!(mesh.model_refs().is_empty());
    assert!(mesh.model_draw_refs().is_empty());
    assert!(mesh.liquid_quads().is_empty());
}

fn meshing(c: &mut Criterion) {
    let assets = terrain_assets();
    let classifier = BlockClassifier::new(AIR);
    let empty = uniform_chunk(AIR);
    let solid = uniform_chunk(1);
    let mixed = mixed_chunk();
    let sparse = chunk_of(sparse_id);
    let light = MeshLightSample::try_new(11, 3).unwrap();
    let sampler = move |_coordinate: [i32; 3]| light;
    let mut group = c.benchmark_group("meshing/terrain");
    for (name, chunk) in [
        ("empty", &empty),
        ("uniform_solid", &solid),
        ("mixed", &mixed),
        ("sparse", &sparse),
    ] {
        let mut neighbourhood = MeshNeighbourhood::new(chunk);
        for offset in OFFSETS {
            assert!(neighbourhood.insert(offset, &empty));
        }
        for lit in [false, true] {
            let output = if lit {
                mesh_sub_chunk_in_neighbourhood_with_lighting(
                    &classifier,
                    &assets,
                    MODE,
                    &neighbourhood,
                    &sampler,
                )
            } else {
                mesh_sub_chunk_in_neighbourhood(&classifier, &assets, MODE, &neighbourhood)
            };
            assert_geometry(&output, chunk);
            match name {
                "empty" => assert!(output.connectivity().is_all_connected()),
                "uniform_solid" => assert!(output.connectivity().is_empty()),
                "sparse" => assert!(output.connectivity().is_all_connected()),
                _ => {
                    assert!(
                        output
                            .connectivity()
                            .is_connected(Face::PositiveX, Face::PositiveY)
                    );
                    assert!(
                        !output
                            .connectivity()
                            .is_connected(Face::NegativeY, Face::PositiveY)
                    );
                }
            }
        }
        group.bench_with_input(
            BenchmarkId::new("full_bright", name),
            &neighbourhood,
            |b, input| {
                b.iter(|| {
                    black_box(mesh_sub_chunk_in_neighbourhood(
                        black_box(&classifier),
                        black_box(&assets),
                        black_box(MODE),
                        black_box(input),
                    ))
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("sampled_light", name),
            &neighbourhood,
            |b, input| {
                b.iter(|| {
                    black_box(mesh_sub_chunk_in_neighbourhood_with_lighting(
                        black_box(&classifier),
                        black_box(&assets),
                        black_box(MODE),
                        black_box(input),
                        black_box(&sampler),
                    ))
                });
            },
        );
    }
    group.finish();
}

fn surrounded_by<'a>(chunk: &'a SubChunk, neighbour: &'a SubChunk) -> MeshNeighbourhood<'a> {
    let mut neighbourhood = MeshNeighbourhood::new(chunk);
    for offset in OFFSETS {
        assert!(neighbourhood.insert(offset, neighbour));
    }
    neighbourhood
}

/// The same terrain in a hashed-id session, where every block resolves through a hash lookup.
fn meshing_hashed(c: &mut Criterion) {
    let sequential = terrain_assets();
    let hashed = hashed_terrain_assets();
    let classifier = BlockClassifier::new(AIR);
    let empty = uniform_chunk(AIR);
    let light = MeshLightSample::try_new(11, 3).unwrap();
    let sampler = move |_coordinate: [i32; 3]| light;
    let [first, second] = HASHED_TERRAIN;
    let mut group = c.benchmark_group("meshing/terrain_hashed");
    for (name, id_at) in [
        ("mixed", mixed_id as fn(u8, u8, u8) -> u32),
        ("sparse", sparse_id),
    ] {
        let sequential_chunk = chunk_of(id_at);
        let hashed_chunk = chunk_with_palette(id_at, [AIR, first, second]);
        let hashed_neighbourhood = surrounded_by(&hashed_chunk, &empty);
        assert_eq!(
            mesh_sub_chunk_in_neighbourhood_with_lighting(
                &classifier,
                &hashed,
                NetworkIdMode::Hashed,
                &hashed_neighbourhood,
                &sampler,
            ),
            mesh_sub_chunk_in_neighbourhood_with_lighting(
                &classifier,
                &sequential,
                MODE,
                &surrounded_by(&sequential_chunk, &empty),
                &sampler,
            ),
        );
        group.bench_with_input(
            BenchmarkId::new("sampled_light", name),
            &hashed_neighbourhood,
            |b, input| {
                b.iter(|| {
                    black_box(mesh_sub_chunk_in_neighbourhood_with_lighting(
                        black_box(&classifier),
                        black_box(&hashed),
                        black_box(NetworkIdMode::Hashed),
                        black_box(input),
                        black_box(&sampler),
                    ))
                });
            },
        );
    }
    group.finish();
}

/// The terrain cubes at ids 1 and 2 and, at id 3, a cable-bus part: a six-quad box two pixels
/// deep against the north face it is authored on.
fn overlay_assets() -> RuntimeAssets {
    let base = RuntimeAssets::diagnostic();
    let mut texture = base.texture_array().clone();
    texture.layers = 1;
    let cube = |material| BlockVisual {
        native_material: assets::native_material::NativeMaterialFacts::Unresolved,
        faces: [material; 6],
        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        kind: VisualKind::Cube,
        support: VisualSupport::VanillaFallback,
        contributor_role: ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
        destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
    };
    let part = BlockVisual {
        flags: BlockFlags::empty(),
        kind: VisualKind::Model,
        model_template: 0,
        ..cube(0)
    };
    let (low, high, depth) = (0, 256, 32);
    let quad = |positions, face| ModelQuad {
        positions,
        uvs: [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
        material: 0,
        flags: face,
    };
    let model_quads = vec![
        quad(
            [
                [low, low, low],
                [high, low, low],
                [high, low, depth],
                [low, low, depth],
            ],
            1,
        ),
        quad(
            [
                [low, high, depth],
                [high, high, depth],
                [high, high, low],
                [low, high, low],
            ],
            2,
        ),
        quad(
            [
                [low, low, depth],
                [low, high, depth],
                [low, high, low],
                [low, low, low],
            ],
            3,
        ),
        quad(
            [
                [high, low, low],
                [high, high, low],
                [high, high, depth],
                [high, low, depth],
            ],
            4,
        ),
        quad(
            [
                [high, low, low],
                [low, low, low],
                [low, high, low],
                [high, high, low],
            ],
            5,
        ),
        quad(
            [
                [low, low, depth],
                [high, low, depth],
                [high, high, depth],
                [low, high, depth],
            ],
            6,
        ),
    ];
    base.with_block_overlay(
        1,
        &BlockOverlay {
            visuals: vec![cube(0), cube(1), part],
            light_properties: vec![LightProperties::OPAQUE_DARK; 3],
            materials: vec![
                Material {
                    texture: TextureRef::new(1, 0).unwrap(),
                    flags: 0,
                    ..Material::unvaried()
                };
                2
            ],
            model_templates: vec![ModelTemplate {
                quad_start: 0,
                quad_count: 6,
                flags: 0,
            }],
            model_quads,
            textures: vec![texture],
            ..Default::default()
        },
    )
    .unwrap()
}

/// The mixed terrain with an AE2-dense layer above it, 128 cable buses each carrying an arm on six
/// faces and a part, against the same terrain without overlays.
fn meshing_overlays(c: &mut Criterion) {
    let assets = overlay_assets();
    let classifier = BlockClassifier::new(AIR);
    let empty = uniform_chunk(AIR);
    let mixed = mixed_chunk();
    let neighbourhood = surrounded_by(&mixed, &empty);
    let light = MeshLightSample::try_new(11, 3).unwrap();
    let sampler = move |_coordinate: [i32; 3]| light;
    let overlays: Vec<_> = (0..16_u8)
        .flat_map(|x| (0..8_u8).map(move |z| [x, 14, z * 2]))
        .flat_map(|local| {
            (0..7_u8).map(move |slot| CellOverlay {
                local,
                face: slot % 6,
                network_value: 3,
            })
        })
        .collect();
    let mesh = |overlays: &[CellOverlay]| {
        mesh_sub_chunk_in_neighbourhood_with_options(
            &classifier,
            &assets,
            MODE,
            &neighbourhood,
            &sampler,
            MeshOptions {
                overlays,
                ..Default::default()
            },
        )
    };
    let bare = mesh(&[]);
    let dressed = mesh(&overlays);
    assert_eq!(dressed.cube_quads(), bare.cube_quads());
    assert_eq!(
        dressed.model_refs().len() - bare.model_refs().len(),
        overlays.len()
    );
    eprintln!(
        "meshing/overlays: {} overlays add {} model references and {} lit quads",
        overlays.len(),
        dressed.model_refs().len() - bare.model_refs().len(),
        dressed.model_lighting().len() - bare.model_lighting().len(),
    );
    let mut group = c.benchmark_group("meshing/overlays");
    for (name, list) in [("without", &[][..]), ("with_896", &overlays[..])] {
        group.bench_with_input(BenchmarkId::new("sampled_light", name), &list, |b, list| {
            b.iter(|| black_box(mesh(black_box(*list))));
        });
    }
    group.finish();
}

fn uniform_biome(id: i32) -> Arc<BiomeStorage> {
    let mut payload = vec![1];
    payload.extend(zig_zag_i32(id));
    DecodedBiomeColumn::decode(0, 1, &payload, &RawBiomeIds { default_biome: 0 })
        .storage(0)
        .unwrap()
}

fn biome_records(c: &mut Criterion) {
    let a = uniform_biome(7);
    let b = uniform_biome(9);
    let uniform: [Option<Arc<BiomeStorage>>; BIOME_NEIGHBOUR_SLOT_COUNT] =
        std::array::from_fn(|_| Some(Arc::clone(&a)));
    // Exact former biome_record_mesh_timing fixture: the east column uses biome 9.
    let boundary =
        std::array::from_fn(|slot| Some(Arc::clone(if slot % 3 == 2 { &b } else { &a })));
    let record = PackedBiomeRecord::from_neighbourhood(&uniform, |id| id);
    assert_eq!(record.uniform_tint_index(), Some(7));
    assert_eq!(
        record.words().len(),
        meshing::biome_lattice::DESCRIPTOR_WORDS + 2
    );
    let record = PackedBiomeRecord::from_neighbourhood(&boundary, |id| id);
    assert_eq!(record.uniform_tint_index(), None);
    eprintln!(
        "BIOME_RECORD boundary_bytes={} uniform_bytes={}",
        record.byte_len(),
        (meshing::biome_lattice::DESCRIPTOR_WORDS + 2) * size_of::<u32>()
    );
    for dz in -1_i8..=1 {
        for dx in -1_i8..=1 {
            let coordinate = [
                if dx < 0 {
                    -1
                } else if dx == 0 {
                    8
                } else {
                    16
                },
                8,
                if dz < 0 {
                    -1
                } else if dz == 0 {
                    8
                } else {
                    16
                },
            ];
            let slot = biome_neighbour_index(dx, dz).unwrap();
            assert_eq!(
                record.tint_index_at(coordinate),
                Some(if slot % 3 == 2 { 9 } else { 7 })
            );
        }
    }
    let east_weight: f32 = record
        .blend_samples([15, 8, 8])
        .unwrap()
        .iter()
        .filter(|sample| sample.tint_index == 9)
        .map(|sample| sample.weight)
        .sum();
    assert!((east_weight - 0.5762481).abs() < 0.00001);
    let mut group = c.benchmark_group("meshing/biome_record");
    for (name, halo) in [("uniform", &uniform), ("boundary", &boundary)] {
        group.bench_with_input(BenchmarkId::from_parameter(name), halo, |b, input| {
            b.iter(|| {
                black_box(PackedBiomeRecord::from_neighbourhood(
                    black_box(input),
                    |id| id,
                ))
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    meshing,
    meshing_hashed,
    meshing_overlays,
    biome_records
);
criterion_main!(benches);
