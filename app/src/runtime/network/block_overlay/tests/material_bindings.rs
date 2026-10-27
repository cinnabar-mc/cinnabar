use serde_json::{Value, json};

use super::*;

/// The generator fixture's pack plus `bindings` as its Cinnabar material bindings.
fn bound_view(bindings: Value) -> LayeredPackView {
    let id = "00000000-0000-0000-0000-000000000001";
    let manifest = json!({"format_version":2,"header":{"uuid":id,"version":[1,0,0]},
        "modules":[{"type":"resources"}]});
    let terrain = json!({"texture_data":{"gen":{"textures":"textures/blocks/gen"}}});
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in [
        ("manifest.json", manifest.to_string().into_bytes()),
        (
            "textures/terrain_texture.json",
            terrain.to_string().into_bytes(),
        ),
        ("textures/blocks/gen.png", png(2, 2, |_, _| [9, 9, 9, 255])),
        ("models/blocks/gen.geo.json", GEOMETRY.as_bytes().to_vec()),
        (
            pack_entity_compiler::MATERIAL_BINDINGS_PATH,
            bindings.to_string().into_bytes(),
        ),
    ] {
        writer
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![
            protocol::ResourcePackArchive::unencrypted(
                id.parse().unwrap(),
                "1.0.0".into(),
                String::new(),
                writer.finish().unwrap().into_inner(),
            ),
        ]),
    ))
}

/// Coverage flags of each distinct generator material the overlay admitted.
fn generator_coverage(bindings: Value) -> Vec<u32> {
    let blocks = CustomBlocks {
        blocks: vec![generator()].into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    let compiled = compile_block_overlay(&bound_view(bindings), &blocks, false, None).unwrap();
    let mut coverage: Vec<_> = compiled.overlay.materials[1..]
        .iter()
        .map(|material| {
            material.flags
                & (assets::MATERIAL_FLAG_ALPHA_BLEND | assets::MATERIAL_FLAG_ALPHA_CUTOUT)
        })
        .collect();
    coverage.sort_unstable();
    coverage.dedup();
    coverage
}

fn glass(binding: Value) -> Value {
    json!({
        "format_version": "1.0.0",
        "materials": {"example:glass": {"material": {"states": ["Blending", "DisableCulling"]}}},
        "bindings": [binding]
    })
}

#[test]
fn a_block_binding_blends_only_the_states_it_names() {
    assert_eq!(generator_coverage(json!({"format_version": "1.0.0"})), [0]);
    assert_eq!(
        generator_coverage(glass(json!({
            "block": "test:generator", "material": "example:glass"
        }))),
        [assets::MATERIAL_FLAG_ALPHA_BLEND]
    );
    assert_eq!(
        generator_coverage(glass(json!({
            "block": "test:generator",
            "states": {"minecraft:cardinal_direction": "west"},
            "material_instance": "*",
            "material": "example:glass"
        }))),
        [0, assets::MATERIAL_FLAG_ALPHA_BLEND]
    );
}

#[test]
fn block_bindings_for_other_blocks_states_or_instances_change_nothing() {
    for binding in [
        json!({"block": "test:other", "material": "example:glass"}),
        json!({"block": "test:generator", "states": {"minecraft:cardinal_direction": "up"},
               "material": "example:glass"}),
        json!({"block": "test:generator", "states": {"unknown:state": 1},
               "material": "example:glass"}),
        json!({"block": "test:generator", "material_instance": "side", "material": "example:glass"}),
    ] {
        assert_eq!(generator_coverage(glass(binding)), [0]);
    }
}
