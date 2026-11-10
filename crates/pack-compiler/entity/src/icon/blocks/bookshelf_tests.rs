use super::*;

#[test]
fn bookshelf_gui_faces_keep_empty_front_artwork_and_authored_caps() {
    let root = tempfile::tempdir().unwrap();
    super::tests::write_pack(
        root.path(),
        r#"{"chiseled_bookshelf":{"textures":{
            "down":"cap","up":"cap","north":"front","south":"back","east":"east","west":"west"
        }}}"#,
        r#"{"texture_data":{
            "cap":{"textures":"textures/blocks/cap"},
            "front":{"textures":["textures/blocks/front","textures/blocks/filled"]},
            "back":{"textures":"textures/blocks/back"},
            "east":{"textures":"textures/blocks/east"},
            "west":{"textures":"textures/blocks/west"}
        }}"#,
        "[]",
    );
    for (name, color) in [
        ("front", [210, 30, 40, 255]),
        ("filled", [10, 220, 40, 255]),
        ("back", [50, 60, 200, 255]),
        ("east", [60, 170, 180, 255]),
        ("west", [190, 70, 180, 255]),
    ] {
        image::RgbaImage::from_pixel(16, 16, image::Rgba(color))
            .save(root.path().join(format!("textures/blocks/{name}.png")))
            .unwrap();
    }
    let cap = image::RgbaImage::from_fn(16, 16, |x, y| image::Rgba([x as u8, y as u8, 90, 255]));
    cap.save(root.path().join("textures/blocks/cap.png"))
        .unwrap();
    let blocks = IconBlocks::read(root.path()).unwrap();
    let mut states = 0;
    for record in blocks.records.iter().filter(|record| {
        record.name.as_ref() == assets::facts::pack::PackBlock::ChiseledBookshelf.identifier()
    }) {
        states += 1;
        let gui = blocks
            .gui_cube_tiles(root.path(), BlockVisualId(record.sequential_id))
            .unwrap()
            .expect("a bookshelf has independent GUI faces");
        for (face, expected) in [
            (BlockFace::South, [210, 30, 40, 255]),
            (BlockFace::North, [50, 60, 200, 255]),
            (BlockFace::West, [60, 170, 180, 255]),
            (BlockFace::East, [190, 70, 180, 255]),
        ] {
            assert_eq!(
                &gui[face as usize].rgba8[..4],
                &expected,
                "{} {face:?}",
                record.canonical_state
            );
        }
        for face in [BlockFace::Up, BlockFace::Down] {
            assert_eq!(
                &*gui[face as usize].rgba8,
                cap.as_raw(),
                "caps retain their authored UV direction"
            );
        }
    }
    assert!(
        states > 1,
        "the fixture covers placed directions and book counts"
    );
    let visual = blocks
        .records
        .iter()
        .find(|record| {
            record.name.as_ref() == assets::facts::pack::PackBlock::ChiseledBookshelf.identifier()
        })
        .unwrap()
        .sequential_id;
    let compiled = crate::compile_entity_pack(vec![("entity/fixture.json".into(),
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"fixture:gui"}}}"#.to_vec())])
        .unwrap().unwrap().assets;
    let mut sprites = Vec::new();
    let baked = crate::icon::bake::run(
        root.path(),
        Some(&RuntimeAssets::diagnostic()),
        Some(&blocks),
        &std::collections::BTreeMap::new(),
        &std::collections::BTreeMap::from([(visual, Err(crate::icon::cube::Reject::Geometry))]),
        &compiled,
        &mut sprites,
    )
    .unwrap();
    assert!(
        baked.block_sheets.is_empty(),
        "a GUI override must not invent a held cube sheet"
    );
    assert_eq!(
        baked.gui_block_sheets.len(),
        1,
        "GUI artwork must be emitted independently of held geometry"
    );
    let thumbnail = baked.model_sprites[&(visual, 0)];
    assert!(
        baked
            .block_models
            .iter()
            .any(|model| { model.sprite == thumbnail && model.visual == BlockVisualId(visual) }),
        "the GUI cube keeps the held world-model binding"
    );
}
