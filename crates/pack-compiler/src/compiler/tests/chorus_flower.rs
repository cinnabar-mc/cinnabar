use super::*;
use assets::{CompiledBiomeAssets, MODEL_TEMPLATE_FLAG_CHORUS, ModelQuad};

const LIVING: u8 = 10;
const WITHERED: u8 = 20;

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
                "minecraft:air" | "minecraft:stone" | "minecraft:chorus_flower"
            )
        })
        .collect::<Vec<_>>();
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    for (path, red) in [
        ("stone", 1),
        ("chorus_flower", LIVING),
        ("chorus_flower_dead", WITHERED),
    ] {
        write_png(
            directory.path().join(format!("textures/blocks/{path}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &[red, 90, 40, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
        );
    }
    write(
        directory.path().join("blocks.json"),
        serde_json::json!({
            "stone": {"textures": "stone"},
            "chorus_flower": {"textures": "chorus_flower"},
        })
        .to_string(),
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::json!({"texture_data": {
            "stone": {"textures": "textures/blocks/stone"},
            "chorus_flower": {"textures": [
                "textures/blocks/chorus_flower",
                "textures/blocks/chorus_flower_dead",
            ]},
        }})
        .to_string(),
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

/// The red channel naming the art a quad samples.
fn art(compiled: &assets::CompiledAssets, quad: &ModelQuad) -> u8 {
    let texture = compiled.materials[quad.material as usize].texture;
    let mip = &compiled.texture_pages[texture.page() as usize].texture.mips[0];
    mip.rgba8[(texture.layer() * mip.size * mip.size * 4) as usize]
}

#[test]
fn chorus_flowers_are_three_bars_with_withered_art_at_their_final_age() {
    let (compiled, records) = fixture();
    let mut seen = 0;
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:chorus_flower")
    {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let expected = if state["age"]["value"].as_u64() == Some(5) {
            WITHERED
        } else {
            LIVING
        };
        let visual = compiled.visuals[record.sequential_id as usize];
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_CHORUS);
        let quads =
            &compiled.model_quads[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(quads.len(), 18, "{}", record.canonical_state);
        for quad in quads {
            assert_eq!(art(&compiled, quad), expected, "{}", record.canonical_state);
        }
        for (axis, bar) in quads.chunks(6).enumerate() {
            for dimension in 0..3 {
                let values = bar
                    .iter()
                    .flat_map(|quad| quad.positions.map(|position| position[dimension]));
                let extent = (values.clone().min().unwrap(), values.max().unwrap());
                let wanted = if dimension == axis {
                    (0, 256)
                } else {
                    (32, 224)
                };
                assert_eq!(extent, wanted, "bar {axis}, dimension {dimension}");
            }
        }
        seen += 1;
    }
    assert_eq!(seen, 6);
}
