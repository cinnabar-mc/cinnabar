use assets::{NetworkIdMode, RuntimeAssets};
use image::{Rgba, RgbaImage};
use std::fs;
use {super::*, assets::BlockFace};

#[test]
fn pots_and_lanterns_keep_cutout_materials_after_carrier_roundtrip() {
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../assets/bedrock-target.json")).unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let records = assets::read_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
        protocol,
    )
    .unwrap()
    .into_iter()
    .filter(|record| {
        matches!(
            record.name.as_ref(),
            "minecraft:air" | "minecraft:stone" | "minecraft:dirt" | "minecraft:flower_pot"
        ) || (record.name.ends_with("lantern")
            && canonical_state_u32(&record.canonical_state, "hanging").is_some())
    })
    .collect::<Vec<_>>();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join("textures/blocks")).unwrap();
    let blocks = records
        .iter()
        .map(|record| {
            let name = record.name.strip_prefix("minecraft:").unwrap();
            let texture = if matches!(name, "stone" | "dirt") {
                "solid"
            } else {
                "model"
            };
            (name.to_owned(), serde_json::json!({"textures": texture}))
        })
        .collect::<serde_json::Map<_, _>>();
    fs::write(
        root.join("blocks.json"),
        serde_json::to_vec(&blocks).unwrap(),
    )
    .unwrap();
    fs::write(root.join("textures/terrain_texture.json"), r#"{"texture_data":{"solid":{"textures":"textures/blocks/solid"},"model":{"textures":"textures/blocks/model"}}}"#).unwrap();
    fs::write(root.join("textures/flipbook_textures.json"), "[]").unwrap();
    RgbaImage::from_pixel(
        assets::TILE_SIZE,
        assets::TILE_SIZE,
        Rgba([90, 65, 40, 255]),
    )
    .save(root.join("textures/blocks/solid.png"))
    .unwrap();
    let mut sprite = RgbaImage::from_pixel(
        assets::TILE_SIZE,
        assets::TILE_SIZE,
        Rgba([180, 95, 40, 255]),
    );
    for x in 0..assets::TILE_SIZE {
        sprite.put_pixel(x, 0, Rgba([0, 0, 0, 0]));
    }
    sprite.save(root.join("textures/blocks/model.png")).unwrap();
    let lights = vec![
        assets::LightProperties::default();
        records
            .iter()
            .map(|record| record.sequential_id as usize + 1)
            .max()
            .unwrap()
    ];
    let (compiled, _) = compile_pack_inner(
        root,
        &records,
        &lights,
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let names = records
        .iter()
        .filter(|record| {
            record.name.as_ref() == "minecraft:flower_pot"
                || canonical_state_u32(&record.canonical_state, "hanging").is_some()
        })
        .map(|record| record.name.as_ref())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(names.contains("minecraft:copper_lantern"));
    for name in names {
        let models = records
            .iter()
            .filter(|record| record.name.as_ref() == name)
            .collect::<Vec<_>>();
        assert!(!models.is_empty(), "fixture must contain {name}");
        for record in models {
            let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
            let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
            let quads = &runtime.model_quads()[template.quad_start as usize
                ..(template.quad_start + template.quad_count) as usize];
            let body = BlockFace::ALL.map(|face| visual.face(face).material_id());
            let surfaces = quads
                .iter()
                .filter(|quad| body.contains(&quad.material))
                .collect::<Vec<_>>();
            assert!(!surfaces.is_empty(), "{name} must contain model surfaces");
            for quad in surfaces {
                let alpha = runtime.material(quad.material).flags
                    & (MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT);
                assert_eq!(
                    alpha, MATERIAL_FLAG_ALPHA_CUTOUT,
                    "{} {} needs alpha testing",
                    record.name, record.canonical_state
                );
                assert_ne!(
                    quad.flags & MODEL_QUAD_FLAG_TWO_SIDED,
                    0,
                    "{} {} must draw both sides of alpha-tested surfaces",
                    record.name,
                    record.canonical_state
                );
            }
        }
    }
}

#[test]
fn every_turtle_egg_state_compiles_its_model_from_the_cracked_art_array() {
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../assets/bedrock-target.json")).unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let records = assets::read_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
        protocol,
    )
    .unwrap()
    .into_iter()
    .filter(|record| {
        matches!(
            record.name.as_ref(),
            "minecraft:air" | "minecraft:stone" | "minecraft:dirt" | "minecraft:turtle_egg"
        )
    })
    .collect::<Vec<_>>();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join("textures/blocks")).unwrap();
    fs::write(
        root.join("blocks.json"),
        r#"{"air":{"textures":"solid"},"stone":{"textures":"solid"},"dirt":{"textures":"solid"},
            "turtle_egg":{"textures":"turtle_egg"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"solid":{"textures":"textures/blocks/solid"},
            "turtle_egg":{"textures":["textures/blocks/egg_0","textures/blocks/egg_1",
            "textures/blocks/egg_2"]}}}"#,
    )
    .unwrap();
    fs::write(root.join("textures/flipbook_textures.json"), "[]").unwrap();
    for (index, name) in ["solid", "egg_0", "egg_1", "egg_2"].into_iter().enumerate() {
        RgbaImage::from_pixel(
            assets::TILE_SIZE,
            assets::TILE_SIZE,
            Rgba([40 * index as u8, 90, 40, 255]),
        )
        .save(root.join(format!("textures/blocks/{name}.png")))
        .unwrap();
    }
    let lights = vec![
        assets::LightProperties::default();
        records
            .iter()
            .map(|record| record.sequential_id as usize + 1)
            .max()
            .unwrap()
    ];
    let (compiled, _) = compile_pack_inner(
        root,
        &records,
        &lights,
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();

    let eggs = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:turtle_egg")
        .collect::<Vec<_>>();
    assert!(!eggs.is_empty(), "the registry must contain turtle eggs");
    let mut art = std::collections::BTreeSet::new();
    for record in eggs {
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert!(
            visual.model_template().is_some(),
            "{} must draw its eggs, not a diagnostic cube",
            record.canonical_state
        );
        art.insert(visual.face(BlockFace::Up).material_id());
    }
    assert_eq!(art.len(), 3, "each cracked state selects its own art");
}
