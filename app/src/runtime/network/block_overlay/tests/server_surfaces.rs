use serde_json::{Value, json};

use super::*;
use crate::runtime::network::block_overlay::CompiledBlockOverlay;
use assets::TextureRef;

/// The generator fixture's pack, its `gen` texture defined by `texture_set` beside `images`.
fn surface_view(texture_set: Value, images: &[(&str, Vec<u8>)]) -> LayeredPackView {
    let id = "00000000-0000-0000-0000-000000000051";
    let manifest = json!({"format_version":2,"header":{"uuid":id,"version":[1,0,0]},
        "modules":[{"type":"resources"}]});
    let terrain = json!({"texture_data":{"gen":{"textures":"textures/blocks/gen"}}});
    let mut files = vec![
        ("manifest.json", manifest.to_string().into_bytes()),
        (
            "textures/terrain_texture.json",
            terrain.to_string().into_bytes(),
        ),
        ("models/blocks/gen.geo.json", GEOMETRY.as_bytes().to_vec()),
        (
            "textures/blocks/gen.texture_set.json",
            texture_set.to_string().into_bytes(),
        ),
    ];
    files.extend(images.iter().map(|(path, bytes)| (*path, bytes.clone())));
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in files {
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

fn compiled(view: &LayeredPackView) -> CompiledBlockOverlay {
    let blocks = CustomBlocks {
        blocks: vec![generator()].into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    compile_block_overlay(view, &blocks, false, None).unwrap()
}

fn color() -> Vec<u8> {
    png(16, 16, |x, _| [40 + x as u8, 90, 140, 255])
}

#[test]
fn texture_set_surfaces_reach_sparse_enhanced_pages_keyed_by_overlay_textures() {
    let view = surface_view(
        json!({"format_version": "1.16.100", "minecraft:texture_set": {
            "color": "gen_color", "normal": "gen_normal",
            "metalness_emissive_roughness": [0, 255, 128]
        }}),
        &[
            ("textures/blocks/gen_color.png", color()),
            (
                "textures/blocks/gen_normal.png",
                png(16, 16, |x, _| [128 + x as u8, 128, 255, 255]),
            ),
        ],
    );
    let overlay = compiled(&view).overlay;
    let maps = overlay.enhanced.expect("the set's surfaces are retained");
    let drawn = overlay.materials[1..]
        .iter()
        .map(|material| material.texture)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(!drawn.is_empty());
    for texture in drawn {
        let reference = maps.material_reference(0, texture);
        assert_ne!(
            reference,
            u32::MAX,
            "the generator's texture has authored surfaces"
        );
        assert_eq!(
            reference >> 31,
            1,
            "a 16-pixel set uses the low-resolution page"
        );
        let flags = assets::texture::PBR_REF_NORMAL | assets::texture::PBR_REF_MATERIAL;
        assert_eq!(reference & flags, flags);
    }
    let base_slot = maps.material_reference(0, TextureRef::new(0, 0).unwrap());
    assert_eq!(
        base_slot,
        u32::MAX,
        "base-carrier slots stay on their fallback"
    );
    let applied = assets::RuntimeAssets::diagnostic()
        .with_block_overlay(1, &compiled(&view).overlay)
        .unwrap();
    assert!(applied.server_enhanced().is_some());
}

#[test]
fn colour_only_sets_and_unreadable_surfaces_keep_only_their_colour() {
    let colour_only = surface_view(
        json!({"format_version": "1.16.100", "minecraft:texture_set": {"color": "gen_color"}}),
        &[("textures/blocks/gen_color.png", color())],
    );
    let compiled_colour_only = compiled(&colour_only);
    assert!(compiled_colour_only.overlay.enhanced.is_none());
    assert_eq!(compiled_colour_only.gaps.downgraded_texture_sets, 0);
    let missing_normal = surface_view(
        json!({"format_version": "1.16.100", "minecraft:texture_set": {
            "color": "gen_color", "normal": "gen_missing"
        }}),
        &[("textures/blocks/gen_color.png", color())],
    );
    let downgraded = compiled(&missing_normal);
    assert!(downgraded.overlay.enhanced.is_none());
    assert_eq!(downgraded.gaps.downgraded_texture_sets, 1);
}
