use super::*;

use assets::facts::pack::PackBlock;

/// Every lectern item draws the terrain stand in the pose whose sloped reading surface drops
/// toward the viewer (+Z), whatever state the item names.
#[test]
fn lectern_inventory_pose_slopes_the_board_toward_the_viewer() {
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
            r.name.as_ref() == "minecraft:lectern" || r.name.as_ref() == PackBlock::Air.identifier()
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
        r#"{"lectern":{"textures":{"up":"top","down":"bottom","north":"front","south":"sides","west":"sides","east":"base"}}}"#,
        r#"{"texture_data":{"top":{"textures":"textures/blocks/top"},"bottom":{"textures":"textures/blocks/bottom"},"front":{"textures":"textures/blocks/front"},"sides":{"textures":"textures/blocks/sides"},"base":{"textures":"textures/blocks/base"}}}"#,
        "[]",
    );
    for (name, pixel) in [
        ("top", [180, 30, 40, 255]),
        ("bottom", [30, 180, 40, 255]),
        ("front", [30, 40, 180, 255]),
        ("sides", [90, 90, 90, 255]),
        ("base", [150, 120, 60, 255]),
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
        .filter(|r| r.name.as_ref() == "minecraft:lectern")
    {
        let pose = blocks.icon_state(BlockVisualId(record.sequential_id));
        let visual = world.resolve(NetworkIdMode::Sequential, pose.0);
        assert_eq!(
            visual.kind(),
            VisualKind::Model,
            "{}",
            record.canonical_state
        );
        let template = world.model_templates()[visual.model_template().unwrap() as usize];
        let quads =
            &world.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        let top = visual.face(BlockFace::Up).material_id();
        let board: Vec<_> = quads.iter().filter(|quad| quad.material == top).collect();
        assert_eq!(board.len(), 1, "one reading surface");
        let corners = board[0].positions;
        let near = corners.iter().map(|p| p[2]).max().unwrap();
        let far = corners.iter().map(|p| p[2]).min().unwrap();
        let height_at = |z: i16| {
            let heights: Vec<_> = corners.iter().filter(|p| p[2] == z).map(|p| p[1]).collect();
            assert_eq!(heights.len(), 2);
            i32::from(heights[0]) + i32::from(heights[1])
        };
        assert!(
            height_at(near) < height_at(far),
            "the board drops toward the viewer: {}",
            record.canonical_state
        );
        seen += 1;
    }
    assert_eq!(seen, 8, "four directions, powered or not");
}
