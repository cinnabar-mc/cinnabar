use serde_json::{Value, json};

use super::*;

/// The generator fixture's pack plus `bindings` as its Cinnabar material bindings.
fn bound_view(bindings: Value) -> LayeredPackView {
    let id = "00000000-0000-0000-0000-000000000001";
    let manifest = json!({"format_version":2,"header":{"uuid":id,"version":[1,0,0]},
        "modules":[{"type":"resources"}]});
    let terrain = json!({"texture_data":{
        "gen":{"textures":"textures/blocks/gen"},
        "gen_glow":{"textures":"textures/blocks/gen_glow"}
    }});
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in [
        ("manifest.json", manifest.to_string().into_bytes()),
        (
            "textures/terrain_texture.json",
            terrain.to_string().into_bytes(),
        ),
        ("textures/blocks/gen.png", png(2, 2, |_, _| [9, 9, 9, 255])),
        (
            "textures/blocks/gen_glow.png",
            png(2, 2, |_, _| [250, 200, 40, 255]),
        ),
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

#[test]
fn block_bindings_give_matching_faces_their_glow_mask() {
    let document = json!({
        "format_version": "1.0.0",
        "materials": {"example:lamp": {
            "textures": {"glow_mask": "gen_glow"},
            "parameters": {"strength": {"scalar": 2.0}}
        }},
        "bindings": [{
            "block": "test:generator",
            "states": {"minecraft:cardinal_direction": "west"},
            "material": "example:lamp"
        }]
    });
    let blocks = CustomBlocks {
        blocks: vec![generator()].into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    let compiled = compile_block_overlay(&bound_view(document), &blocks, false, None).unwrap();
    let masked: Vec<_> = compiled.overlay.materials[1..]
        .iter()
        .filter(|material| material.glow_mask != assets::NO_MASK)
        .collect();
    assert_eq!(
        masked.len(),
        1,
        "only the bound state's material carries the mask"
    );
    assert_eq!(f32::from_bits(masked[0].mask_strength), 2.0);
    assert_eq!(masked[0].emissive_mask, assets::NO_MASK);
    assert!(
        compiled.overlay.materials[1..]
            .iter()
            .any(|material| material.glow_mask == assets::NO_MASK),
        "other states keep the unmasked material"
    );
}

#[test]
fn a_block_binding_condition_selects_states_like_a_permutation_condition() {
    let bound = |condition: &str| {
        generator_coverage(glass(json!({
            "block": "test:generator",
            "condition": condition,
            "material": "example:glass"
        })))
    };
    assert_eq!(
        bound("q.block_state('minecraft:cardinal_direction') == 'west'"),
        [0, assets::MATERIAL_FLAG_ALPHA_BLEND]
    );
    assert_eq!(bound("1.0"), [assets::MATERIAL_FLAG_ALPHA_BLEND]);
    assert_eq!(bound("0.0"), [0]);
    assert_eq!(
        bound("q.block_state('unknown:state') == 1"),
        [0],
        "a condition that cannot evaluate does not apply"
    );
}

#[test]
fn bindings_matching_nothing_and_unresolved_masks_are_counted_once() {
    let document = json!({
        "format_version": "1.0.0",
        "materials": {
            "example:lamp": {"textures": {"glow_mask": "missing_key"}},
            "example:glass": {"material": {"states": ["Blending"]}}
        },
        "bindings": [
            {"block": "test:generator", "material": "example:lamp"},
            {"block": "test:generator", "material_instance": "*", "material": "example:lamp"},
            {"block": "test:other", "material": "example:glass"}
        ]
    });
    let blocks = CustomBlocks {
        blocks: vec![generator()].into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    let compiled = compile_block_overlay(&bound_view(document), &blocks, false, None).unwrap();
    assert_eq!(compiled.gaps.unmatched_material_bindings, 1);
    assert_eq!(
        compiled.gaps.unresolved_binding_masks, 1,
        "one key, however many faces name it"
    );
}

/// The generator compiled with `bindings` as its Cinnabar material bindings.
fn compiled_generator(bindings: Value) -> super::super::CompiledBlockOverlay {
    let blocks = CustomBlocks {
        blocks: vec![generator()].into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    compile_block_overlay(&bound_view(bindings), &blocks, false, None).unwrap()
}

#[test]
fn the_last_matching_binding_decides_both_method_and_masks() {
    let document = |order: [&str; 2]| {
        json!({
            "format_version": "1.0.0",
            "materials": {
                "example:lamp": {"textures": {"glow_mask": "gen_glow"}},
                "example:glass": {"material": {"states": ["Blending", "DisableCulling"]}}
            },
            "bindings": order.map(|material| json!({"block": "test:generator", "material": material}))
        })
    };
    let glass_last = compiled_generator(document(["example:lamp", "example:glass"]));
    for material in &glass_last.overlay.materials[1..] {
        assert_ne!(material.flags & assets::MATERIAL_FLAG_ALPHA_BLEND, 0);
        assert_eq!(
            material.glow_mask,
            assets::NO_MASK,
            "the glass names no mask"
        );
    }
    let lamp_last = compiled_generator(document(["example:glass", "example:lamp"]));
    for material in &lamp_last.overlay.materials[1..] {
        assert_eq!(material.flags & assets::MATERIAL_FLAG_ALPHA_BLEND, 0);
        assert_ne!(material.glow_mask, assets::NO_MASK);
    }
}

#[test]
fn many_bindings_for_other_blocks_leave_a_blocks_own_binding_intact() {
    const OTHERS: usize = 20_000;
    let mut bindings: Vec<_> = (0..OTHERS)
        .map(|index| json!({"block": format!("test:other_{index}"), "material": "example:glass"}))
        .collect();
    bindings.push(json!({
        "block": "test:generator",
        "states": {"minecraft:cardinal_direction": "west"},
        "material": "example:glass"
    }));
    let document = json!({
        "format_version": "1.0.0",
        "materials": {"example:glass": {"material": {"states": ["Blending", "DisableCulling"]}}},
        "bindings": bindings
    });
    assert_eq!(
        generator_coverage(document.clone()),
        [0, assets::MATERIAL_FLAG_ALPHA_BLEND]
    );
    assert_eq!(
        compiled_generator(document)
            .gaps
            .unmatched_material_bindings,
        OTHERS as u32
    );
}
