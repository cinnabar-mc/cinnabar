use super::*;
use assets::{
    BlockFace, CompiledBiomeAssets, LightProperties, MODEL_TEMPLATE_FLAG_COMPOUND_NEXT,
    NetworkIdMode, RuntimeAssets,
};

/// Compiles every dried ghast state from a pack whose tentacle art uses `tentacle_alpha`.
fn compile_ghasts(tentacle_alpha: u8) -> (RuntimeAssets, Vec<assets::RegistryRecord>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let registry = assets::read_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
        protocol,
    )
    .unwrap();
    let records = registry
        .into_vec()
        .into_iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:air" | "minecraft:dried_ghast"
            )
        })
        .collect::<Vec<_>>();

    let directory = tempfile::tempdir().unwrap();
    let faces = ["back", "front", "left", "right", "top", "bottom"];
    let mut terrain = serde_json::Map::new();
    for face in faces {
        let art = (1..=4)
            .map(|level| format!("textures/blocks/ghast_{face}_{level}"))
            .collect::<Vec<_>>();
        terrain.insert(
            format!("dried_ghast_{face}"),
            serde_json::json!({ "textures": art }),
        );
    }
    terrain.insert(
        "dried_ghast_tentacles".to_owned(),
        serde_json::json!({ "textures": [
            "textures/blocks/ghast_tentacles_1", "textures/blocks/ghast_tentacles_2",
            "textures/blocks/ghast_tentacles_3", "textures/blocks/ghast_tentacles_4",
        ]}),
    );
    write(
        directory.path().join("blocks.json"),
        r#"{"dried_ghast":{"carried_textures":"dried_ghast_tentacles","textures":{
            "down":"dried_ghast_bottom","east":"dried_ghast_left","north":"dried_ghast_back",
            "south":"dried_ghast_front","up":"dried_ghast_top","west":"dried_ghast_right"}}}"#,
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::json!({ "texture_data": terrain }).to_string(),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let art = faces.into_iter().chain(["tentacles"]);
    for (index, name) in art
        .flat_map(|name| (1..=4).map(move |level| format!("ghast_{name}_{level}")))
        .enumerate()
    {
        let alpha = if name.contains("tentacles") {
            tentacle_alpha
        } else {
            255
        };
        write_png(
            directory.path().join(format!("textures/blocks/{name}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &[index as u8 * 6, 90, 40, alpha].repeat((TILE_SIZE * TILE_SIZE) as usize),
        );
    }
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &vec![LightProperties::default(); span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    (runtime, records)
}

#[test]
fn every_dried_ghast_state_compiles_all_its_faces_from_the_rehydration_art_arrays() {
    let (runtime, records) = compile_ghasts(255);
    let ghasts = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:dried_ghast")
        .collect::<Vec<_>>();
    assert!(!ghasts.is_empty(), "the registry must contain dried ghasts");
    let mut art = std::collections::BTreeSet::new();
    for record in ghasts {
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        let mut template = visual.model_template().unwrap_or_else(|| {
            panic!(
                "{} must draw its body, not a diagnostic cube",
                record.canonical_state
            )
        }) as usize;
        let mut quads = 0;
        loop {
            let part = runtime.model_templates()[template];
            quads += part.quad_count;
            if part.flags & MODEL_TEMPLATE_FLAG_COMPOUND_NEXT == 0 {
                break;
            }
            template += 1;
        }
        assert_eq!(quads, 42, "{} loses faces", record.canonical_state);
        art.insert(visual.face(BlockFace::Up).material_id());
    }
    assert_eq!(art.len(), 4, "each rehydration level selects its own art");
}

#[test]
fn transparent_tentacle_art_stays_on_the_tentacles() {
    let (runtime, records) = compile_ghasts(0);
    let mut seen = 0;
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:dried_ghast")
    {
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        let parts = assets::model_template_parts(
            runtime.model_templates(),
            visual.model_template().unwrap(),
        )
        .unwrap();
        let body = BlockFace::ALL.map(|face| visual.face(face).material_id());
        let tentacle = parts
            .iter()
            .flat_map(|part| {
                let start = part.quad_start as usize;
                &runtime.model_quads()[start..start + part.quad_count as usize]
            })
            .any(|quad| !body.contains(&quad.material));
        assert!(
            tentacle,
            "{} replaced its tentacle art with body art",
            record.canonical_state
        );
        seen += 1;
        let held = assets::gui_item::block_item_quads(
            visual.kind(),
            body,
            visual.model_template(),
            runtime.model_templates(),
            runtime.model_quads(),
        )
        .unwrap();
        assert_eq!(held.len(), 42, "the held model reads every part");
    }
    assert!(seen > 0, "the registry must contain dried ghasts");
}
