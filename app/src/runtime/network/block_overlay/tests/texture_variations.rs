use serde_json::json;

use super::*;

/// A pack whose `gen` terrain entry has two weighted variations.
fn varied_view() -> LayeredPackView {
    let id = "00000000-0000-0000-0000-000000000001";
    let manifest = json!({"format_version":2,"header":{"uuid":id,"version":[1,0,0]},
        "modules":[{"type":"resources"}]});
    let terrain = json!({"texture_data":{"gen":{"textures":{"variations":[
        {"path":"textures/blocks/gen_a","weight":1},
        {"path":"textures/blocks/gen_b","weight":3}
    ]}}}});
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in [
        ("manifest.json", manifest.to_string().into_bytes()),
        (
            "textures/terrain_texture.json",
            terrain.to_string().into_bytes(),
        ),
        (
            "textures/blocks/gen_a.png",
            png(16, 16, |_, _| [200, 0, 0, 255]),
        ),
        (
            "textures/blocks/gen_b.png",
            png(16, 16, |_, _| [0, 0, 200, 255]),
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

/// Variation counts of the materials a full cube textured with `gen` admits.
fn variation_counts(packed_bools: u8) -> Vec<u32> {
    let mut instance = CustomMaterialInstance::with_authored_defaults("*", "gen");
    instance.packed_bools = packed_bools;
    let cube = block(
        "test:varied",
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("minecraft:geometry.full_block".into()),
                materials: Some(Box::new([instance])),
                ..CustomVisualComponents::default()
            },
            ..CustomBlockVisuals::default()
        },
    );
    let blocks = CustomBlocks {
        blocks: vec![cube].into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    let compiled = compile_block_overlay(&varied_view(), &blocks, false, None).unwrap();
    compiled.overlay.materials[1..]
        .iter()
        .map(|material| material.variation_count)
        .collect()
}

#[test]
fn enabled_variations_select_weighted_positional_textures() {
    let counts = variation_counts(1 | 4);
    assert!(
        counts.contains(&2),
        "a two-way positional selector: {counts:?}"
    );
}

#[test]
fn disabled_variations_keep_the_first_texture() {
    assert!(variation_counts(1).iter().all(|&count| count == 0));
}
