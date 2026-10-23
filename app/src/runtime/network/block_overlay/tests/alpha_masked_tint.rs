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
