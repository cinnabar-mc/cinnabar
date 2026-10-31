use super::*;
use assets::{CompiledBiomeAssets, MODEL_QUAD_FLAG_TWO_SIDED, ModelQuad};

const ART: [(&str, u8); 3] = [("plant", 10), ("side", 20), ("top", 30)];

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
                    | "minecraft:azalea"
                    | "minecraft:flowering_azalea"
            )
        })
        .collect::<Vec<_>>();
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut texture_data = serde_json::json!({
        "stone": {"textures": "textures/blocks/stone"},
    });
    write_png(
        directory.path().join("textures/blocks/stone.png"),
        TILE_SIZE,
        TILE_SIZE,
        &[1, 1, 1, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
    );
    for (name, red) in ART {
        let path = format!("textures/blocks/azalea_{name}");
        write_png(
            directory.path().join(format!("{path}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &[red, 90, 40, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
        );
        texture_data[format!("azalea_{name}")] = serde_json::json!({"textures": path});
    }
    let bush = serde_json::json!({"textures": {
        "down": "stone",
        "east": "azalea_plant",
        "north": "azalea_side",
        "south": "stone",
        "up": "azalea_top",
        "west": "stone",
    }});
    write(
        directory.path().join("blocks.json"),
        serde_json::json!({
            "stone": {"textures": "stone"},
            "azalea": bush,
            "flowering_azalea": bush,
        })
        .to_string(),
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

/// The red channel naming the art a quad samples.
fn art(compiled: &assets::CompiledAssets, quad: &ModelQuad) -> u8 {
    let texture = compiled.materials[quad.material as usize].texture;
    let mip = &compiled.texture_pages[texture.page() as usize].texture.mips[0];
    mip.rgba8[(texture.layer() * mip.size * mip.size * 4) as usize]
}

#[test]
fn azalea_bushes_cross_their_stem_through_a_floorless_canopy() {
    let (compiled, records) = fixture();
    let mut seen = 0;
    for record in records
        .iter()
        .filter(|record| record.name.ends_with("azalea"))
    {
        let visual = compiled.visuals[record.sequential_id as usize];
        let template = compiled.model_templates[visual.model_template as usize];
        let quads =
            &compiled.model_quads[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(quads.len(), 7);
        assert!(
            quads
                .iter()
                .all(|quad| quad.flags & MODEL_QUAD_FLAG_TWO_SIDED != 0)
        );
        let (stem, canopy) = quads.split_at(2);
        for quad in stem {
            assert_eq!(art(&compiled, quad), ART[0].1);
            let xs = quad.positions.map(|position| position[0]);
            assert_eq!((xs.iter().min(), xs.iter().max()), (Some(&13), Some(&243)));
        }
        let mut sides = 0;
        let mut tops = 0;
        for quad in canopy {
            let ys = quad.positions.map(|position| position[1]);
            assert_ne!(
                ys, [0; 4],
                "the canopy has no floor: {}",
                record.canonical_state
            );
            let face = quad.flags & assets::MODEL_QUAD_FLAG_FACE_MASK;
            assert_eq!(quad.flags >> 4 & assets::MODEL_QUAD_FLAG_FACE_MASK, face);
            if ys == [256; 4] {
                assert_eq!(art(&compiled, quad), ART[2].1);
                tops += 1;
            } else {
                assert_eq!(art(&compiled, quad), ART[1].1);
                sides += 1;
            }
        }
        assert_eq!((sides, tops), (4, 1));
        seen += 1;
    }
    assert!(seen >= 2);
}
