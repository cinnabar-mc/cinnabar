const RENDER_INVISIBLE_NAMES: [&str; 5] = [
    "minecraft:barrier",
    "minecraft:structure_void",
    "minecraft:invisible_bedrock",
    "minecraft:light_block_0",
    "minecraft:light_block_15",
];

struct CompiledInvisibleFixture {
    assets: RuntimeAssets,
    air: NetworkValues,
    stone: NetworkValues,
    invisible: Vec<(&'static str, NetworkValues)>,
}

fn compiled_invisible_fixture() -> &'static CompiledInvisibleFixture {
    static FIXTURE: OnceLock<CompiledInvisibleFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let records = read_registry(include_bytes!("../../../../assets/data/block-registry-v1001.bin"))
            .expect("decode invisible-block registry");
        let find = |name: &str| {
            records
                .iter()
                .find(|record| record.name.as_ref() == name)
                .unwrap_or_else(|| panic!("{name} record"))
                .clone()
        };
        let values = |record: &RegistryRecord| NetworkValues {
            sequential: record.sequential_id,
            hashed: record.network_hash,
        };
        let air = find("minecraft:air");
        let stone = records
            .iter()
            .find(|record| {
                record.name.as_ref() == "minecraft:stone"
                    && record.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
            })
            .expect("stone full cube")
            .clone();
        let invisible = RENDER_INVISIBLE_NAMES.map(find);

        // The pack textures every block so only the compiler's rule can keep them undrawn.
        let directory = tempfile::tempdir().expect("invisible-block fixture directory");
        let root = directory.path();
        fs::create_dir_all(root.join("textures/blocks")).expect("create fixture tree");
        let routes = std::iter::once("stone")
            .chain(RENDER_INVISIBLE_NAMES.map(|name| name.strip_prefix("minecraft:").unwrap()))
            .map(|name| format!(r#""{name}":{{"textures":"cube"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        fs::write(root.join("blocks.json"), format!("{{{routes}}}")).expect("write blocks");
        fs::write(
            root.join("textures/terrain_texture.json"),
            r#"{"texture_data":{"cube":{"textures":"textures/blocks/cube"}}}"#,
        )
        .expect("write terrain routing");
        fs::write(root.join("textures/flipbook_textures.json"), "[]").expect("write flipbooks");
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[255; 16 * 16 * 4], 16, 16, ExtendedColorType::Rgba8)
            .expect("encode fixture PNG");
        fs::write(root.join("textures/blocks/cube.png"), png).expect("write fixture PNG");

        let mut selected = vec![air.clone(), stone.clone()];
        selected.extend(invisible.iter().cloned());
        let compiled = compile_pack(root, &selected).expect("compile invisible-block fixture");
        let assets = RuntimeAssets::decode(&encode_blob(&compiled).expect("encode fixture"))
            .expect("decode invisible-block fixture");
        CompiledInvisibleFixture {
            assets,
            air: values(&air),
            stone: values(&stone),
            invisible: RENDER_INVISIBLE_NAMES
                .into_iter()
                .zip(invisible.iter().map(values))
                .collect(),
        }
    })
}

fn mesh_invisible_fixture(mode: NetworkIdMode, placements: &[([u8; 3], u32)]) -> ChunkMesh {
    let fixture = compiled_invisible_fixture();
    let air = fixture.air.for_mode(mode);
    let mut palette = vec![air];
    let indexed = placements
        .iter()
        .map(|&(coordinate, value)| {
            let index = palette.iter().position(|&id| id == value).unwrap_or_else(|| {
                palette.push(value);
                palette.len() - 1
            });
            (coordinate, index)
        })
        .collect::<Vec<_>>();
    let center = sub_chunk(vec![packed_storage(2, &palette, &indexed)]);
    mesh_sub_chunk(
        &BlockClassifier::new(air),
        &fixture.assets,
        mode,
        &Neighbourhood::default(),
        &center,
    )
}

/// A pack texture for these blocks must not give them terrain geometry.
#[test]
fn render_invisible_blocks_emit_no_terrain_geometry() {
    let fixture = compiled_invisible_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for &(name, values) in &fixture.invisible {
            let mesh = mesh_invisible_fixture(mode, &[([8, 8, 8], values.for_mode(mode))]);
            assert!(mesh.is_empty(), "{name} {mode:?} emitted terrain geometry");
        }
    }
}

/// A stone enclosed by these blocks keeps all six faces.
#[test]
fn render_invisible_blocks_never_cull_neighbour_faces() {
    let fixture = compiled_invisible_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for &(name, values) in &fixture.invisible {
            let invisible = values.for_mode(mode);
            let mut placements = vec![([8, 8, 8], fixture.stone.for_mode(mode))];
            placements.extend(
                [[7, 8, 8], [9, 8, 8], [8, 7, 8], [8, 9, 8], [8, 8, 7], [8, 8, 9]]
                    .map(|coordinate| (coordinate, invisible)),
            );
            let mesh = mesh_invisible_fixture(mode, &placements);
            assert_eq!(mesh.cube_quads().len(), 6, "{name} {mode:?} culled stone faces");
            assert!(mesh.model_refs().is_empty(), "{name} {mode:?} emitted a model");
        }
    }
}

/// The marker layer each fixture block draws into, or `None` for a block drawn in no layer.
fn expected_marker(name: &str) -> Option<assets::TerrainMarker> {
    match name {
        "minecraft:barrier" => Some(assets::TerrainMarker::Barrier),
        "minecraft:structure_void" => Some(assets::TerrainMarker::StructureVoid),
        "minecraft:invisible_bedrock" => None,
        _ => Some(assets::TerrainMarker::Light),
    }
}

/// Barriers, light blocks and structure voids record their cell, layer, block and light, which
/// the renderer draws while their layer is shown; invisible bedrock records nothing.
#[test]
fn marker_blocks_record_their_cell_layer_and_art() {
    let fixture = compiled_invisible_fixture();
    let full_bright = meshing::FULL_BRIGHT_BLOCK_LIGHT | (meshing::FULL_BRIGHT_SKY_LIGHT << 4);
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for &(name, values) in &fixture.invisible {
            let mesh = mesh_invisible_fixture(mode, &[([3, 9, 12], values.for_mode(mode))]);
            let markers = mesh.terrain_markers();
            let Some(expected) = expected_marker(name) else {
                assert!(markers.is_empty(), "{name} {mode:?} recorded a marker");
                continue;
            };
            assert_eq!(markers.len(), 1, "{name} {mode:?}");
            assert_eq!(markers[0].local(), [3, 9, 12], "{name} {mode:?}");
            assert_eq!(markers[0].marker(), expected, "{name} {mode:?}");
            assert_eq!(markers[0].block(), values.sequential, "{name} {mode:?}");
            assert_eq!(markers[0].light(), full_bright, "{name} {mode:?}");
        }
    }
}

/// A section filled with one marker block records every cell.
#[test]
fn a_uniform_marker_section_records_every_cell() {
    let fixture = compiled_invisible_fixture();
    let (_, barrier) = fixture.invisible[0];
    let mode = NetworkIdMode::Sequential;
    let center = sub_chunk(vec![uniform_storage(barrier.for_mode(mode))]);
    let mesh = mesh_sub_chunk(
        &BlockClassifier::new(fixture.air.for_mode(mode)),
        &fixture.assets,
        mode,
        &Neighbourhood::default(),
        &center,
    );
    assert!(mesh.is_empty());
    assert_eq!(mesh.terrain_markers().len(), 16 * 16 * 16);
    assert!(
        mesh.terrain_markers()
            .iter()
            .all(|marker| marker.marker() == assets::TerrainMarker::Barrier)
    );
}
