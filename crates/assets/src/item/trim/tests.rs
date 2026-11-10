use super::*;
use std::collections::BTreeMap;

/// Builds a one-row texture from original test pixels.
fn image(path: &str, width: u16, pixels: &[[u8; 4]]) -> EquipmentTexture {
    EquipmentTexture {
        identifier: path.into(),
        width,
        height: 1,
        rgba8: pixels.iter().flatten().copied().collect::<Vec<_>>().into(),
    }
}

#[test]
fn palette_substitution_keeps_holes_and_dyes_only_the_base() {
    let base = image(
        "textures/models/armor/leather_1",
        3,
        &[[200, 200, 200, 255], [200, 200, 200, 255], [0; 4]],
    );
    let textures = BTreeMap::from([
        (
            "textures/trims/sentry",
            image(
                "mask",
                3,
                &[[100, 100, 100, 0], [100, 100, 100, 255], [100, 100, 100, 0]],
            ),
        ),
        (
            "textures/trims/color_palettes/trim_palette",
            image("palette", 1, &[[100, 100, 100, 255]]),
        ),
        (
            "textures/trims/color_palettes/gold",
            image("gold", 1, &[[240, 170, 20, 255]]),
        ),
    ]);
    let output = compose(
        &base,
        Some(ArmourMaterial::Leather),
        "gold",
        "sentry",
        ArmorSlot::Chestplate,
        false,
        Some(Dye {
            rgb: [0, 255, 0],
            alpha: DyeAlpha::Coverage,
        }),
        None,
        |path| textures.get(path),
    )
    .unwrap();
    assert_eq!(&output.rgba8[..4], &[0, 200, 0, 255]);
    assert_eq!(&output.rgba8[4..8], &[240, 170, 20, 255]);
    assert_eq!(&output.rgba8[8..], &[0; 4]);
    assert!(
        compose(
            &base,
            Some(ArmourMaterial::Leather),
            "absent",
            "sentry",
            ArmorSlot::Chestplate,
            false,
            None,
            None,
            |path| textures.get(path)
        )
        .is_none()
    );
}

#[test]
fn trim_dye_distinguishes_coverage_from_texture_opacity() {
    let base = image("base", 2, &[[200, 200, 200, 64]; 2]);
    let textures = BTreeMap::from([
        (
            "textures/trims/sentry",
            image("mask", 2, &[[100, 100, 100, 0], [100, 100, 100, 255]]),
        ),
        (
            "textures/trims/color_palettes/trim_palette",
            image("palette", 1, &[[100, 100, 100, 255]]),
        ),
        (
            "textures/trims/color_palettes/gold",
            image("gold", 1, &[[240, 170, 20, 255]]),
        ),
    ]);
    for (alpha, expected) in [(DyeAlpha::Coverage, 255), (DyeAlpha::Preserve, 64)] {
        let output = compose(
            &base,
            Some(ArmourMaterial::Leather),
            "gold",
            "sentry",
            ArmorSlot::Chestplate,
            false,
            Some(Dye {
                rgb: [0, 255, 0],
                alpha,
            }),
            None,
            |path| textures.get(path),
        )
        .unwrap();
        assert_eq!(&output.rgba8[..4], &[149, 200, 149, expected]);
        assert_eq!(&output.rgba8[4..], &[240, 170, 20, 255]);
    }
}

#[test]
fn matching_materials_leggings_item_masks_and_authored_aliases_select_their_own_art() {
    let base = image("textures/models/armor/diamond_2", 1, &[[20, 30, 40, 255]]);
    let palette = image("palette", 1, &[[100, 100, 100, 255]]);
    let mut textures = BTreeMap::from([
        (
            "textures/trims/sentry_leggings",
            image("legs", 1, &[[100, 100, 100, 255]]),
        ),
        (
            "textures/trims/items/leggings_trim",
            image("icon", 1, &[[100, 100, 100, 255]]),
        ),
        ("textures/trims/color_palettes/trim_palette", palette),
        (
            "textures/trims/color_palettes/diamond",
            image("light", 1, &[[30, 250, 250, 255]]),
        ),
        (
            "textures/trims/color_palettes/diamond_darker",
            image("dark", 1, &[[10, 100, 100, 255]]),
        ),
    ]);
    for icon in [false, true] {
        assert_eq!(
            compose(
                &base,
                Some(ArmourMaterial::Diamond),
                "diamond",
                "sentry",
                ArmorSlot::Leggings,
                icon,
                None,
                None,
                |path| textures.get(path)
            )
            .unwrap()
            .rgba8
            .as_ref(),
            &[10, 100, 100, 255]
        );
    }
    let mut overrides = crate::equipment::EquipmentResources::default();
    overrides
        .textures
        .insert("diamond_palette".into(), "custom/palette".into());
    textures.insert("custom/palette", image("custom", 1, &[[90, 30, 200, 255]]));
    assert_eq!(
        compose(
            &base,
            Some(ArmourMaterial::Diamond),
            "diamond",
            "sentry",
            ArmorSlot::Leggings,
            false,
            None,
            Some(&overrides),
            |path| textures.get(path)
        )
        .unwrap()
        .rgba8
        .as_ref(),
        &[90, 30, 200, 255]
    );
}

#[test]
fn matching_material_palette_ignores_reskinned_texture_paths() {
    let textures = BTreeMap::from([
        (
            "textures/trims/sentry",
            image("mask", 1, &[[100, 100, 100, 255]]),
        ),
        (
            "textures/trims/items/chestplate_trim",
            image("icon", 1, &[[100, 100, 100, 255]]),
        ),
        (
            "textures/trims/color_palettes/trim_palette",
            image("palette", 1, &[[100, 100, 100, 255]]),
        ),
        (
            "textures/trims/color_palettes/diamond",
            image("light", 1, &[[30, 250, 250, 255]]),
        ),
        (
            "textures/trims/color_palettes/diamond_darker",
            image("dark", 1, &[[10, 100, 100, 255]]),
        ),
    ]);
    let facts = crate::facts::ItemFacts::builtin().unwrap();
    for (identifier, expected) in [
        ("minecraft:diamond_chestplate", [10, 100, 100, 255]),
        ("minecraft:iron_chestplate", [30, 250, 250, 255]),
    ] {
        for path in ["textures/custom/coat", "textures/models/armor/diamond_1"] {
            let base = image(path, 1, &[[20, 30, 40, 255]]);
            for icon in [false, true] {
                let output = compose(
                    &base,
                    facts.by_identifier(identifier).unwrap().armor_material(),
                    "diamond",
                    "sentry",
                    ArmorSlot::Chestplate,
                    icon,
                    None,
                    None,
                    |path| textures.get(path),
                )
                .unwrap();
                assert_eq!(
                    output.rgba8.as_ref(),
                    expected,
                    "{identifier} {path} {icon}"
                );
            }
        }
    }
}

#[test]
fn measured_alpha_matches_composed_alpha_across_sizes() {
    let base = image("base", 3, &[[9, 9, 9, 0], [9, 9, 9, 128], [9, 9, 9, 255]]);
    let mask = EquipmentTexture {
        identifier: "mask".into(),
        width: 2,
        height: 2,
        rgba8: [
            [100, 100, 100, 0],
            [100, 100, 100, 64],
            [100, 100, 100, 255],
            [7, 7, 7, 200],
        ]
        .concat()
        .into(),
    };
    let textures = BTreeMap::from([
        ("textures/trims/items/chestplate_trim", mask),
        (
            "textures/trims/color_palettes/trim_palette",
            image("palette", 1, &[[100, 100, 100, 255]]),
        ),
        (
            "textures/trims/color_palettes/gold",
            image("gold", 1, &[[240, 170, 20, 255]]),
        ),
    ]);
    let layers = layers(
        None,
        "gold",
        "",
        ArmorSlot::Chestplate,
        true,
        None,
        |path| textures.get(path),
    )
    .unwrap();
    let composed = layers.compose(&base, None).unwrap();
    let size = [base.width, base.height];
    assert_eq!(layers.size(size), [composed.width, composed.height]);
    assert_eq!([composed.width, composed.height], [3, 2]);
    for y in 0..composed.height {
        for x in 0..composed.width {
            let measured = layers.alpha(
                size,
                |x, y| base.rgba8[(y * usize::from(base.width) + x) * 4 + 3],
                x,
                y,
            );
            let at = (usize::from(y) * usize::from(composed.width) + usize::from(x)) * 4 + 3;
            assert_eq!(measured, composed.rgba8[at], "texel {x},{y}");
        }
    }
}
