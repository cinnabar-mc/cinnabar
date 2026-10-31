use super::*;
use assets::{CompiledBiomeAssets, MODEL_TEMPLATE_FLAG_TWISTING_VINE};
use assets::{MODEL_TEMPLATE_FLAG_WEEPING_VINE, ModelTemplate, VisualKind};

const STEM_RED: u8 = 40;
const TIP_RED: u8 = 200;

/// Compiles both vines, either on their own art or both on the weeping vine's art.
fn fixture(shared_art: bool) -> (assets::CompiledAssets, Vec<RegistryRecord>) {
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
                "minecraft:air" | "minecraft:weeping_vines" | "minecraft:twisting_vines"
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
    for vine in ["weeping_vines", "twisting_vines"] {
        for (part, red) in [("base", STEM_RED), ("bottom", TIP_RED)] {
            let path = format!("textures/blocks/{vine}_{part}");
            write_png(
                directory.path().join(format!("{path}.png")),
                TILE_SIZE,
                TILE_SIZE,
                &[red, 90, 40, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
            );
            texture_data.insert(
                format!("{vine}_{part}"),
                serde_json::json!({"textures": path}),
            );
        }
        let art = if shared_art { "weeping_vines" } else { vine };
        let base = format!("{art}_base");
        blocks.insert(
            vine.to_owned(),
            serde_json::json!({"textures": {
                "down": format!("{art}_bottom"),
                "east": base,
                "north": base,
                "south": base,
                "up": base,
                "west": base,
            }}),
        );
    }
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

fn quad_reds(compiled: &assets::CompiledAssets, template: ModelTemplate) -> Vec<u8> {
    let start = template.quad_start as usize;
    compiled.model_quads[start..start + template.quad_count as usize]
        .iter()
        .map(|quad| {
            let texture = compiled.materials[quad.material as usize].texture;
            let mip = &compiled.texture_pages[texture.page() as usize].texture.mips[0];
            mip.rgba8[(texture.layer() * mip.size * mip.size * 4) as usize]
        })
        .collect()
}

#[test]
fn vines_compile_stem_planes_before_tip_planes_at_plant_width() {
    let (compiled, records) = fixture(false);
    assert_vine_templates(&compiled, &records);
}

/// A shared template would make one vine read the other's neighbour for its continuation.
#[test]
fn vines_on_shared_art_keep_their_own_growth_direction() {
    let (compiled, records) = fixture(true);
    assert_vine_templates(&compiled, &records);
}

fn assert_vine_templates(compiled: &assets::CompiledAssets, records: &[RegistryRecord]) {
    assets::encode_blob(compiled).expect("vine templates pass blob validation");
    let mut seen = [0; 2];
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() != "minecraft:air")
    {
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Cross, "{}", record.canonical_state);
        let template = compiled.model_templates[visual.model_template as usize];
        let weeping = record.name.as_ref() == "minecraft:weeping_vines";
        assert_eq!(
            template.flags,
            if weeping {
                MODEL_TEMPLATE_FLAG_WEEPING_VINE
            } else {
                MODEL_TEMPLATE_FLAG_TWISTING_VINE
            }
        );
        assert_eq!(
            quad_reds(compiled, template),
            [STEM_RED, STEM_RED, TIP_RED, TIP_RED]
        );
        let start = template.quad_start as usize;
        for quad in &compiled.model_quads[start..start + 4] {
            let xs = quad.positions.map(|position| position[0]);
            assert_eq!((xs.iter().min(), xs.iter().max()), (Some(&13), Some(&243)));
        }
        seen[usize::from(weeping)] += 1;
    }
    assert!(seen.iter().all(|&states| states > 0));
}
