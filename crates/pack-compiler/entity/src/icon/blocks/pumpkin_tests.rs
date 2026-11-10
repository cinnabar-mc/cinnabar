use super::*;

#[test]
fn pumpkin_gui_and_held_fronts_keep_their_class_artwork_in_the_carried_pose() {
    assert_pumpkin_cube_fronts(3);
}

#[test]
fn pumpkin_gui_and_held_fronts_use_first_entry_when_the_class_variant_is_missing() {
    assert_pumpkin_cube_fronts(2);
}

/// Checks pumpkin cube faces for a pack with the supplied front array length.
fn assert_pumpkin_cube_fronts(front_count: usize) {
    let root = tempfile::tempdir().unwrap();
    let records = read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let pumpkins: Vec<_> = records
        .iter()
        .filter(|record| {
            assets::facts::pack::face_slots(&record.name)
                == Some(assets::facts::pack::FaceSlots::Pumpkin)
        })
        .collect();
    let catalog = pumpkins
        .iter()
        .map(|record| {
            (
                record.name.strip_prefix("minecraft:").unwrap().to_owned(),
                serde_json::json!({"textures": {
                    "up": "cap", "down": "cap", "south": "front",
                    "north": "side", "east": "side", "west": "side"
                }}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let front_textures = [
        "textures/blocks/carved",
        "textures/blocks/lit",
        "textures/blocks/side",
    ];
    super::tests::write_pack(
        root.path(),
        &serde_json::Value::Object(catalog).to_string(),
        &serde_json::json!({"texture_data":{
            "cap":{"textures":"textures/blocks/cap"},
            "side":{"textures":"textures/blocks/side"},
            "front":{"textures": &front_textures[..front_count]}
        }})
        .to_string(),
        "[]",
    );
    for (name, color) in [
        ("cap", [90, 80, 70, 255]),
        ("side", [20, 30, 40, 255]),
        ("carved", [240, 10, 20, 255]),
        ("lit", [10, 240, 20, 255]),
    ] {
        image::RgbaImage::from_pixel(16, 16, image::Rgba(color))
            .save(root.path().join(format!("textures/blocks/{name}.png")))
            .unwrap();
    }
    let blocks = IconBlocks::read(root.path()).unwrap();
    assert!(!pumpkins.is_empty());
    for record in pumpkins {
        let visual = BlockVisualId(record.sequential_id);
        let held = blocks
            .carried_cube_tiles(root.path(), visual)
            .unwrap()
            .unwrap();
        let gui = blocks.gui_cube_tiles(root.path(), visual).unwrap().unwrap();
        let shown = &blocks.records[blocks.icon_state(visual).0 as usize];
        assert!(matches!(
            shown.property(assets::facts::BlockProperty::MinecraftCardinalDirection),
            Some(assets::facts::StateValue::String(direction)) if direction.as_ref() == "south"
        ));
        let expected = match (record.name.as_ref(), front_count) {
            ("minecraft:carved_pumpkin", _) | ("minecraft:pumpkin", 2) => [240, 10, 20, 255],
            ("minecraft:lit_pumpkin", _) => [10, 240, 20, 255],
            ("minecraft:pumpkin", _) => [20, 30, 40, 255],
            _ => panic!("unclassified pumpkin class"),
        };
        assert_eq!(
            &gui[BlockFace::South as usize].rgba8[..4],
            &expected,
            "{} {}",
            record.name,
            record.canonical_state
        );
        assert_eq!(
            &gui[BlockFace::West as usize].rgba8[..4],
            &[20, 30, 40, 255]
        );
        assert_eq!(&gui[BlockFace::Up as usize].rgba8[..4], &[90, 80, 70, 255]);
        assert_eq!(
            &held[BlockFace::South as usize].rgba8[..4],
            &expected,
            "held {} {}",
            record.name,
            record.canonical_state
        );
        assert_eq!(&held[BlockFace::Up as usize].rgba8[..4], &[90, 80, 70, 255]);
        assert_eq!(
            &held[BlockFace::West as usize].rgba8[..4],
            &[20, 30, 40, 255]
        );
    }
}

#[test]
fn pumpkin_inventory_icons_face_south_whichever_state_the_item_routes_to() {
    let root = tempfile::tempdir().unwrap();
    let blocks = IconBlocks::read(root.path()).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for record in blocks.records.iter().filter(|record| {
        matches!(
            assets::facts::pack::face_slots(&record.name),
            Some(assets::facts::pack::FaceSlots::Pumpkin)
        )
    }) {
        let pose =
            &blocks.records[blocks.icon_state(BlockVisualId(record.sequential_id)).0 as usize];
        let state: serde_json::Value = serde_json::from_str(&pose.canonical_state).unwrap();
        assert_eq!(pose.name, record.name);
        assert_eq!(
            state["minecraft:cardinal_direction"]["value"], "south",
            "{} {}",
            record.name, record.canonical_state
        );
        seen.insert(record.name.as_ref());
    }
    assert_eq!(
        seen.into_iter().collect::<Vec<_>>(),
        [
            "minecraft:carved_pumpkin",
            "minecraft:lit_pumpkin",
            "minecraft:pumpkin"
        ]
    );
}
