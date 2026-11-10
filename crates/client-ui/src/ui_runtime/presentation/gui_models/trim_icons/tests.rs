use super::*;

#[test]
fn unchanged_preview_trims_reuse_compositions_and_pages() {
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let texture = |identifier: &str, pixel: [u8; 4]| assets::EquipmentTexture {
        identifier: identifier.into(),
        width: 1,
        height: 1,
        rgba8: Arc::from(pixel),
    };
    let catalog = assets::RuntimeEquipmentCatalog::from_parts(
        [7; 32],
        Vec::new(),
        vec![
            texture("textures/trims/sentry", [100, 100, 100, 255]),
            texture(
                "textures/trims/color_palettes/trim_palette",
                [100, 100, 100, 255],
            ),
            texture("textures/trims/color_palettes/gold", [240, 170, 20, 255]),
        ],
    )
    .unwrap();
    presentation.set_equipment_catalog(Some(Arc::new(catalog)));
    let base = player_preview::PreviewTexture {
        width: 1,
        height: 1,
        rgba: Arc::from([10, 20, 30, 255]),
        tint: None,
    };
    let trim = protocol::ItemTrim {
        material: "gold".into(),
        pattern: "sentry".into(),
    };
    let compose = |presentation: &UiPresentationRuntime| {
        presentation
            .trim_preview(
                &base,
                "textures/models/armor/diamond_1",
                None,
                &trim,
                "minecraft:diamond_chestplate",
            )
            .unwrap()
    };
    presentation.gui_models.trim_icons.begin_frame();
    let first = compose(&presentation);
    presentation.finish_trim_art();
    let region = presentation.gui_models.trim_icons.region(&first).unwrap();
    let pages = presentation.gui_models.trim_icons.pages();
    presentation.gui_models.trim_icons.begin_frame();
    let again = compose(&presentation);
    presentation.finish_trim_art();
    assert!(Arc::ptr_eq(&first.rgba, &again.rgba));
    assert_eq!(
        presentation.gui_models.trim_icons.region(&again),
        Some(region)
    );
    assert_eq!(
        pages[0].identity(),
        presentation.gui_models.trim_icons.pages()[0].identity()
    );
    presentation.gui_models.trim_icons.begin_frame();
    presentation.finish_trim_art();
    assert!(presentation.gui_models.trim_icons.region(&first).is_none());
    assert!(presentation.gui_models.trim_icons.pages().is_empty());
}

#[test]
fn shared_preview_pixels_keep_the_admitted_armor_material() {
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let texture = |identifier: &str, pixel: [u8; 4]| assets::EquipmentTexture {
        identifier: identifier.into(),
        width: 1,
        height: 1,
        rgba8: Arc::from(pixel),
    };
    presentation.set_equipment_catalog(Some(Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            [7; 32],
            Vec::new(),
            vec![
                texture("textures/trims/sentry", [100, 100, 100, 255]),
                texture(
                    "textures/trims/color_palettes/trim_palette",
                    [100, 100, 100, 255],
                ),
                texture("textures/trims/color_palettes/diamond", [30, 250, 250, 255]),
                texture(
                    "textures/trims/color_palettes/diamond_darker",
                    [10, 100, 100, 255],
                ),
            ],
        )
        .unwrap(),
    )));
    let base = player_preview::PreviewTexture {
        width: 1,
        height: 1,
        rgba: Arc::from([20, 30, 40, 255]),
        tint: None,
    };
    let trim = protocol::ItemTrim {
        material: "diamond".into(),
        pattern: "sentry".into(),
    };
    for (identifier, expected) in [
        ("minecraft:diamond_chestplate", [10, 100, 100, 255]),
        ("minecraft:iron_chestplate", [30, 250, 250, 255]),
        ("minecraft:diamond_chestplate", [10, 100, 100, 255]),
    ] {
        let preview = presentation
            .trim_preview(&base, "textures/custom/coat", None, &trim, identifier)
            .unwrap();
        assert_eq!(preview.rgba.as_ref(), expected, "{identifier}");
    }
}
