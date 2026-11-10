use super::*;
use assets::{BlockFace, NetworkIdMode, RuntimeAssets};

/// Compiles every bell state from a pack that names only the frame's two faces.
fn compile_bells() -> (RuntimeAssets, Vec<RegistryRecord>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let registry = assets::read_registry_for_protocol(&bytes, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["light_registry"].as_str().unwrap())).unwrap(),
        &bytes,
        registry.len(),
        protocol,
    )
    .unwrap();
    let records: Vec<_> = registry
        .into_vec()
        .into_iter()
        .filter(|r| matches!(r.name.as_ref(), "minecraft:air" | "minecraft:bell"))
        .collect();
    let directory = tempfile::tempdir().unwrap();
    let mut definitions = serde_json::Map::new();
    let mut textures = serde_json::Map::new();
    for (index, face) in ["west", "east", "down", "up", "north", "south"]
        .into_iter()
        .enumerate()
    {
        // The frame reads only these two faces, so the other faces' art may be unresolved.
        if matches!(face, "west" | "east") {
            definitions.insert(face.into(), serde_json::json!(face));
        }
        textures.insert(
            face.into(),
            serde_json::json!({"textures": format!("textures/{face}")}),
        );
        let pixels: Vec<u8> = (0..TILE_SIZE * TILE_SIZE)
            .flat_map(|_| [40 + index as u8 * 30, 90, 120, 255])
            .collect();
        write_png(
            directory.path().join(format!("textures/{face}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &pixels,
        );
    }
    write(
        directory.path().join("blocks.json"),
        serde_json::to_vec(&serde_json::json!({"bell": {"textures": definitions}})).unwrap(),
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::to_vec(&serde_json::json!({"texture_data": textures})).unwrap(),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let span = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        assets::CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let bytes = assets::encode_blob(&compiled).unwrap();
    (RuntimeAssets::decode(&bytes).unwrap(), records)
}

#[test]
fn bell_frames_are_terrain_models_in_every_mounting() {
    let (runtime, records) = compile_bells();
    let mut count = 0;
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:bell")
    {
        count += 1;
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(
            visual.kind(),
            VisualKind::Model,
            "terrain draws the frame: {}",
            record.canonical_state
        );
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        let materials = BlockFace::ALL.map(|face| visual.face(face).material_id());
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let (legs, wood) = (
            materials[BlockFace::West as usize],
            materials[BlockFace::East as usize],
        );
        let expected = match state["attachment"]["value"].as_str().unwrap() {
            "standing" => [12, 6],
            _ => [0, 6],
        };
        let used =
            [legs, wood].map(|material| quads.iter().filter(|q| q.material == material).count());
        assert_eq!(
            used, expected,
            "stone legs and wooden bars: {}",
            record.canonical_state
        );
    }
    assert_eq!(
        count, 32,
        "four mountings, four directions and the ring toggle"
    );
}
