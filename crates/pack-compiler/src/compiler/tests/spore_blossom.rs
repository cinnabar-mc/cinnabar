use super::*;
use assets::{CompiledBiomeAssets, MODEL_QUAD_FLAG_TWO_SIDED, ModelQuad};

const PETAL: u8 = 10;
const BASE: u8 = 20;

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
                "minecraft:air" | "minecraft:stone" | "minecraft:spore_blossom"
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
        ("spore_blossom", PETAL),
        ("spore_blossom_base", BASE),
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
            "spore_blossom": {"textures": {
                "down": "spore_blossom",
                "side": "spore_blossom_base",
                "up": "spore_blossom_base",
            }},
        })
        .to_string(),
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::json!({"texture_data": {
            "stone": {"textures": "textures/blocks/stone"},
            "spore_blossom": {"textures": "textures/blocks/spore_blossom"},
            "spore_blossom_base": {"textures": "textures/blocks/spore_blossom_base"},
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
fn spore_blossoms_droop_four_petals_past_their_cell_below_a_flat_base() {
    let (compiled, records) = fixture();
    let record = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:spore_blossom")
        .unwrap();
    let visual = compiled.visuals[record.sequential_id as usize];
    let template = compiled.model_templates[visual.model_template as usize];
    let quads =
        &compiled.model_quads[template.quad_start as usize..][..template.quad_count as usize];
    assert_eq!(quads.len(), 5);
    let (petals, base) = quads.split_at(4);
    for (index, petal) in petals.iter().enumerate() {
        assert_eq!(art(&compiled, petal), PETAL);
        assert_ne!(petal.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
        // Every petal rises from its tip at the art's top row to the centre at its bottom row.
        for (position, uv) in petal.positions.iter().zip(petal.uvs) {
            let expected = if position[1] == 120 { 0 } else { 4096 };
            assert_eq!(uv[1], expected, "petal {index}");
            assert!(position[1] == 120 || position[1] == 248);
        }
        let axis = usize::from(index < 2) * 2;
        for position in &petal.positions {
            let reach = position[axis];
            if position[1] == 120 {
                assert!(reach == -128 || reach == 384, "petal {index}");
            } else {
                assert_eq!(reach, 128, "petal {index}");
            }
        }
    }
    assert_eq!(art(&compiled, &base[0]), BASE);
    assert_eq!(base[0].positions.map(|position| position[1]), [254; 4]);
}
