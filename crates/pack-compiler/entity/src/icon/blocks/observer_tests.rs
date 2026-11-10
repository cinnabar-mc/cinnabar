use super::*;

use assets::facts::pack::PackBlock;

const OBSERVER: PackBlock = PackBlock::Observer;

/// The observer item is a carried cube whose south face shows the authored front, like the
/// base game's item drawn in its south-facing pose.
#[test]
fn observer_inventory_faces_show_the_front_toward_the_viewer() {
    let target: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../../../../assets/bedrock-target.json"
    ))
    .unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let registry =
        std::fs::read(repo.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let all = read_registry_for_protocol(&registry, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        &std::fs::read(repo.join(target["artifacts"]["light_registry"].as_str().unwrap())).unwrap(),
        &registry,
        all.len(),
        protocol,
    )
    .unwrap();
    let records: Vec<_> = all
        .into_vec()
        .into_iter()
        .filter(|r| {
            [OBSERVER, PackBlock::Air, PackBlock::Stone]
                .iter()
                .any(|block| r.name.as_ref() == block.identifier())
        })
        .collect();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("biomes")).unwrap();
    std::fs::write(
        root.path().join("biomes/plains.client_biome.json"),
        r#"{"minecraft:client_biome":{"description":{"identifier":"minecraft:plains"}}}"#,
    )
    .unwrap();
    std::fs::write(root.path().join("biomes/plains.biome.json"), r#"{"minecraft:biome":{"description":{"identifier":"minecraft:plains"},"components":{"minecraft:climate":{"temperature":0.5,"downfall":0.5}}}}"#).unwrap();
    std::fs::create_dir_all(root.path().join("textures/colormap")).unwrap();
    for map in assets::TintMapId::ALL {
        image::RgbImage::from_pixel(
            assets::TINT_MAP_SIZE,
            assets::TINT_MAP_SIZE,
            image::Rgb([100, 150, 100]),
        )
        .save(
            root.path()
                .join(format!("textures/colormap/{}.png", map.source_name())),
        )
        .unwrap();
    }
    super::tests::write_pack(
        root.path(),
        r#"{"stone":{"textures":"top"},"observer":{"textures":{"up":"top","down":"top","north":"front","south":"back","west":"side","east":"side"}}}"#,
        r#"{"texture_data":{"top":{"textures":"textures/blocks/top"},"front":{"textures":"textures/blocks/front"},"back":{"textures":"textures/blocks/back"},"side":{"textures":"textures/blocks/side"}}}"#,
        "[]",
    );
    for (name, pixel) in [
        ("top", [180, 30, 40, 255]),
        ("front", [30, 180, 40, 255]),
        ("back", [30, 40, 180, 255]),
        ("side", [90, 90, 90, 255]),
    ] {
        image::RgbaImage::from_pixel(16, 16, image::Rgba(pixel))
            .save(root.path().join(format!("textures/blocks/{name}.png")))
            .unwrap();
    }
    let span = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let compiled = pack_compiler::compile_pack_with_biomes(
        root.path(),
        root.path(),
        &records,
        &[],
        &lights[..span],
        protocol,
    )
    .unwrap();
    let world = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let blocks = IconBlocks::read(root.path()).unwrap();
    let mut seen = 0;
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == OBSERVER.identifier())
    {
        let visual = BlockVisualId(record.sequential_id);
        assert!(
            blocks.is_carried_cube(&world, visual),
            "{}",
            record.canonical_state
        );
        let tiles = blocks
            .carried_cube_tiles(root.path(), visual)
            .unwrap()
            .unwrap();
        for (face, pixel) in [
            (BlockFace::South, [30, 180, 40, 255]),
            (BlockFace::North, [30, 40, 180, 255]),
            (BlockFace::Up, [180, 30, 40, 255]),
            (BlockFace::West, [90, 90, 90, 255]),
        ] {
            assert_eq!(
                &tiles[face as usize].rgba8[..4],
                &pixel,
                "{face:?} of {}",
                record.canonical_state
            );
        }
        seen += 1;
    }
    assert!(seen > 1);
}
