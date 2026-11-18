use super::*;

/// Flags of the materials a full cube textured with `lucky` admits.
fn material_flags(tint_method: Option<&str>, packed_bools: u8) -> Vec<u32> {
    let mut instance = CustomMaterialInstance::with_authored_defaults("*", "lucky");
    instance.tint_method = tint_method.map(Into::into);
    instance.packed_bools = packed_bools;
    let cube = block(
        "test:tinted",
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
    let compiled = compile_block_overlay(&view(), &blocks, false, None).unwrap();
    compiled.overlay.materials[1..]
        .iter()
        .map(|material| material.flags)
        .collect()
}

#[test]
fn alpha_masked_tint_weights_a_tint_by_texture_alpha() {
    let masked = |flags: &u32| flags & assets::MATERIAL_FLAG_OVERLAY_MASK != 0;
    assert!(material_flags(Some("grass"), 1 | 8).iter().any(masked));
    assert!(!material_flags(Some("grass"), 1).iter().any(masked));
    // Without a tint there is nothing to mask.
    assert!(!material_flags(None, 1 | 8).iter().any(masked));
}

#[test]
fn every_tint_method_selects_its_biome_tint_and_unknown_methods_draw_untinted() {
    let tint = |method: Option<&str>| {
        let flags = material_flags(method, 1);
        assert!(!flags.is_empty());
        flags[0] & (assets::MATERIAL_FLAG_TINT_MASK | assets::MATERIAL_FLAG_FOLIAGE_CLASS_MASK)
    };
    let foliage = assets::MATERIAL_FLAG_FOLIAGE_TINT;
    for (method, expected) in [
        (None, 0),
        (Some("none"), 0),
        (Some("default_foliage"), foliage),
        (
            Some("birch_foliage"),
            foliage | assets::MATERIAL_FLAG_BIRCH_FOLIAGE,
        ),
        (
            Some("evergreen_foliage"),
            foliage | assets::MATERIAL_FLAG_EVERGREEN_FOLIAGE,
        ),
        (
            Some("dry_foliage"),
            foliage | assets::MATERIAL_FLAG_DRY_FOLIAGE,
        ),
        (Some("grass"), assets::MATERIAL_FLAG_GRASS_TINT),
        (Some("water"), assets::MATERIAL_FLAG_WATER_TINT),
        // Bedrock looks names up exactly; anything else is none.
        (Some("Grass"), 0),
        (Some("swamp_foliage"), 0),
    ] {
        assert_eq!(tint(method), expected, "{method:?}");
    }
}
