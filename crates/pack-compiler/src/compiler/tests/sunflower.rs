use super::*;
use assets::CompiledBiomeAssets;
use assets::{MODEL_QUAD_FLAG_TWO_SIDED, ModelQuad};

const TOP: u8 = 10;
const BOTTOM: u8 = 20;
const FRONT: u8 = 30;
const BACK: u8 = 40;

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
                "minecraft:air" | "minecraft:sunflower"
            )
        })
        .collect::<Vec<_>>();
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path().join("blocks.json"),
        r#"{"sunflower":{"textures":{"down":"sunflower_bottom","side":"sunflower_additional","up":"sunflower_top"}}}"#,
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{
            "sunflower_top":{"textures":"textures/blocks/top"},
            "sunflower_bottom":{"textures":"textures/blocks/bottom"},
            "sunflower_additional":{"textures":["textures/blocks/front","textures/blocks/back"]}
        }}"#,
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    for (name, red) in [
        ("top", TOP),
        ("bottom", BOTTOM),
        ("front", FRONT),
        ("back", BACK),
    ] {
        write_png(
            directory.path().join(format!("textures/blocks/{name}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &[red, 90, 40, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
        );
    }
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

fn half(
    compiled: &assets::CompiledAssets,
    records: &[RegistryRecord],
    upper: u64,
) -> Vec<ModelQuad> {
    let record = records
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:sunflower"
                && serde_json::from_str::<serde_json::Value>(&record.canonical_state).unwrap()
                    ["upper_block_bit"]["value"]
                    .as_u64()
                    == Some(upper)
        })
        .unwrap();
    let visual = compiled.visuals[record.sequential_id as usize];
    let template = compiled.model_templates[visual.model_template as usize];
    let start = template.quad_start as usize;
    compiled.model_quads[start..start + template.quad_count as usize].to_vec()
}

fn art(compiled: &assets::CompiledAssets, material: u32) -> u8 {
    let texture = compiled.materials[material as usize].texture;
    let mip = &compiled.texture_pages[texture.page() as usize].texture.mips[0];
    mip.rgba8[(texture.layer() * mip.size * mip.size * 4) as usize]
}

#[test]
fn sunflower_upper_half_draws_its_tilted_head_over_the_stalk() {
    let (compiled, records) = fixture();
    let quads = half(&compiled, &records, 1);
    assert_eq!(quads.len(), 4);
    assert_eq!(art(&compiled, quads[0].material), TOP);
    assert_eq!(art(&compiled, quads[1].material), TOP);
    let (front, back) = (quads[2], quads[3]);
    assert_eq!(art(&compiled, front.material), FRONT);
    assert_eq!(art(&compiled, back.material), BACK);
    for head in [front, back] {
        assert_eq!(head.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
        let foot = head.positions[0];
        let top = head.positions[2];
        assert_eq!((foot[0], foot[1]), (192, 26));
        assert_eq!((top[0], top[1]), (115, 282));
        assert_eq!(head.uvs[0][1], 4096, "the foot reads the art's bottom");
        assert_eq!(head.uvs[2][1], 0, "the top reads the art's top");
    }
    // The front's winding points its visible side east, the back's west.
    let normal_x = |quad: &ModelQuad| {
        let [a, b, c, _] = quad.positions.map(|p| p.map(i32::from));
        let (u, v) = (
            [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
            [c[0] - b[0], c[1] - b[1], c[2] - b[2]],
        );
        u[1] * v[2] - u[2] * v[1]
    };
    assert!(normal_x(&front) > 0);
    assert!(normal_x(&back) < 0);
}

#[test]
fn sunflower_lower_half_stays_a_plain_stalk() {
    let (compiled, records) = fixture();
    let quads = half(&compiled, &records, 0);
    assert_eq!(quads.len(), 2);
    assert!(
        quads
            .iter()
            .all(|quad| art(&compiled, quad.material) == BOTTOM)
    );
}
