use super::*;
use assets::{
    CompiledBiomeAssets, MODEL_QUAD_FLAG_TWO_SIDED, NetworkIdMode, RuntimeAssets, VisualKind,
};

fn is_record(record: &RegistryRecord) -> bool {
    matches!(
        record.name.as_ref(),
        "minecraft:pointed_dripstone" | "minecraft:sulfur_spike"
    )
}

const THICKNESSES: [&str; 5] = ["tip", "frustum", "middle", "base", "merge"];

/// Red channel of one block's art for a thickness and direction, unique across the fixture.
fn marker(block: usize, thickness: usize, hanging: bool) -> u8 {
    (10 + block * 40 + thickness * 7 + usize::from(hanging) * 3) as u8
}

fn fixture() -> (assets::CompiledAssets, Vec<RegistryRecord>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let registry_bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let registry = assets::read_registry_for_protocol(&registry_bytes, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["light_registry"].as_str().unwrap())).unwrap(),
        &registry_bytes,
        registry.len(),
        protocol,
    )
    .unwrap();
    let records = registry
        .into_vec()
        .into_iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:air"
                    | "minecraft:stone"
                    | "minecraft:pointed_dripstone"
                    | "minecraft:sulfur_spike"
            )
        })
        .collect::<Vec<_>>();
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut texture_data = serde_json::Map::new();
    let mut blocks = serde_json::Map::new();
    for (block_index, block) in ["pointed_dripstone", "sulfur_spike"]
        .into_iter()
        .enumerate()
    {
        let key = |thickness: &str| format!("{block}_{thickness}");
        for (thickness_index, thickness) in THICKNESSES.into_iter().enumerate() {
            let paths = [true, false].map(|hanging| {
                let direction = if hanging { "down" } else { "up" };
                let path = format!("textures/blocks/{block}_{direction}_{thickness}");
                let red = marker(block_index, thickness_index, hanging);
                write_png(
                    directory.path().join(format!("{path}.png")),
                    TILE_SIZE,
                    TILE_SIZE,
                    &[red, 90, 40, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
                );
                path
            });
            texture_data.insert(key(thickness), serde_json::json!({"textures": paths}));
        }
        blocks.insert(
            block.to_owned(),
            serde_json::json!({"textures": {
                "down": key("frustum"),
                "east": key("base"),
                "north": key("tip"),
                "south": key("middle"),
                "up": key("base"),
                "west": key("merge"),
            }}),
        );
    }
    // Provisional fallback records need the canonical stone material to exist.
    write_png(
        directory.path().join("textures/blocks/stone.png"),
        TILE_SIZE,
        TILE_SIZE,
        &[1, 1, 1, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
    );
    texture_data.insert(
        "stone".to_owned(),
        serde_json::json!({"textures": "textures/blocks/stone"}),
    );
    blocks.insert("stone".to_owned(), serde_json::json!({"textures": "stone"}));
    write(
        directory.path().join("blocks.json"),
        serde_json::Value::Object(blocks).to_string(),
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::json!({"texture_data": texture_data}).to_string(),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (compiled, records)
}

#[test]
fn speleothems_cross_the_sprite_for_their_thickness_and_direction() {
    let (compiled, records) = fixture();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let mut seen = 0;
    for record in records.iter().filter(|r| is_record(r)) {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let thickness = state["dripstone_thickness"]["value"].as_str().unwrap();
        let hanging = state["hanging"]["value"].as_u64().unwrap() != 0;
        let block = usize::from(record.name.as_ref() == "minecraft:sulfur_spike");
        let thickness = THICKNESSES.iter().position(|t| *t == thickness).unwrap();

        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(
            visual.kind(),
            VisualKind::Cross,
            "{}",
            record.canonical_state
        );
        let id = visual.model_template().unwrap();
        let template = compiled.model_templates[id as usize];
        assert_eq!(template.quad_count, 2);
        let start = template.quad_start as usize;
        for quad in &compiled.model_quads[start..start + 2] {
            let texture = compiled.materials[quad.material as usize].texture;
            let mip = &compiled.texture_pages[texture.page() as usize].texture.mips[0];
            let red = mip.rgba8[(texture.layer() * mip.size * mip.size * 4) as usize];
            assert_eq!(
                red,
                marker(block, thickness, hanging),
                "{} {}",
                record.name,
                record.canonical_state
            );
            assert_ne!(quad.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
            let xs = quad.positions.map(|p| p[0]);
            assert_eq!(xs.iter().min(), Some(&13));
            assert_eq!(xs.iter().max(), Some(&243));
            let ys = quad.positions.map(|p| p[1]);
            assert_eq!((ys.iter().min(), ys.iter().max()), (Some(&0), Some(&256)));
        }
        seen += 1;
    }
    assert_eq!(seen, 20);
}

#[test]
fn speleothems_slide_around_their_column_by_the_narrow_offset() {
    let (compiled, records) = fixture();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let record = records
        .iter()
        .find(|r| r.name.as_ref() == "minecraft:pointed_dripstone")
        .unwrap();
    let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
    let offset = runtime
        .model_random_offset(visual.model_template().unwrap())
        .unwrap();
    assert_eq!(
        offset.axes.map(|axis| axis.range),
        [[-0.125, 0.125], [0.0, 0.0], [-0.125, 0.125]]
    );
}
