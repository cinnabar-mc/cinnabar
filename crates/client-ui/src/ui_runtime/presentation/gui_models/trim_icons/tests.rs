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
    let trim = render_api::ItemTrim {
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
    let trim = render_api::ItemTrim {
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

/// A presentation with a server chestplate icon and original item trim artwork; the session
/// declares `fixture:coat` as trimmable armor with `art`, or as plain armor without it.
fn custom_armor_presentation(art: Option<protocol::CustomArmorTrim>) -> UiPresentationRuntime {
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let texture = |identifier: &str, pixels: &[u8]| assets::EquipmentTexture {
        identifier: identifier.into(),
        width: (pixels.len() / 4) as u16,
        height: 1,
        rgba8: Arc::from(pixels),
    };
    presentation.set_equipment_catalog(Some(Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            [7; 32],
            Vec::new(),
            vec![
                texture(
                    "textures/trims/items/chestplate_trim",
                    &[100, 100, 100, 255, 0, 0, 0, 0],
                ),
                texture(
                    "textures/trims/color_palettes/trim_palette",
                    &[100, 100, 100, 255],
                ),
                texture(
                    "textures/trims/color_palettes/diamond",
                    &[30, 250, 250, 255],
                ),
                texture(
                    "textures/trims/color_palettes/diamond_darker",
                    &[10, 100, 100, 255],
                ),
                texture(
                    "textures/trims/color_palettes/fixture_gold",
                    &[1, 2, 3, 255],
                ),
                texture("textures/trims/color_palettes/gold", &[240, 170, 20, 255]),
            ],
        )
        .unwrap(),
    )));
    let icons = Arc::new(crate::ui_runtime::presentation::SessionIcons {
        icons: vec![crate::ui_runtime::presentation::SessionIcon {
            identifier: "fixture:coat".into(),
            metadata: 0,
            width: 2,
            height: 1,
            rgba8: vec![10, 20, 30, 255, 40, 50, 60, 255].into(),
        }],
        ..Default::default()
    });
    super::super::super::session_icons::observe(&mut presentation, Some(&icons));
    let items = crate::ui_runtime::item_facts::SessionItemComponents::from_iter([(
        Arc::from("fixture:coat"),
        protocol::ItemComponents {
            icon: Some("fixture_coat".into()),
            wearable_slot: Some("slot.armor.chest".into()),
            armor_trim: art.map(Arc::new),
            ..Default::default()
        },
    )]);
    presentation
        .gui_models
        .trim_icons
        .observe_session_items(Some(&Arc::new(items)));
    presentation
}

/// The first two texels of the dropped-item sprite for a `material` trim on `fixture:coat`.
fn custom_world_texels(presentation: &UiPresentationRuntime, material: &str) -> Option<Vec<u8>> {
    let icon = presentation.item_icon("fixture:coat", 0)?;
    let trim = render_api::ItemTrim {
        material: material.into(),
        pattern: "sentry".into(),
    };
    let sprite = presentation
        .world_trim_artwork(&Arc::from("fixture:coat"), icon, Some(&trim), false)?
        .sprite(presentation, 16)?;
    Some(sprite.rgba8[..8].to_vec())
}

#[test]
fn trimmable_custom_armor_icons_draw_the_stack_trim() {
    let untagged = custom_armor_presentation(None);
    assert_eq!(custom_world_texels(&untagged, "diamond"), None);

    let art = protocol::CustomArmorTrim {
        icon_mask: None,
        icon_palettes: [(
            Arc::from("gold"),
            Arc::from("textures/trims/color_palettes/fixture_gold"),
        )]
        .into_iter()
        .collect(),
    };
    let mut presentation = custom_armor_presentation(Some(art.clone()));
    // Custom armor has no armor material, so a diamond trim keeps the ordinary palette.
    assert_eq!(
        custom_world_texels(&presentation, "minecraft:diamond").as_deref(),
        Some(&[30, 250, 250, 255, 40, 50, 60, 255][..])
    );
    // The icon's own `gold_palette` replaces the material palette.
    assert_eq!(
        custom_world_texels(&presentation, "gold").as_deref(),
        Some(&[1, 2, 3, 255, 40, 50, 60, 255][..])
    );
    let trim = render_api::ItemTrim {
        material: "gold".into(),
        pattern: "sentry".into(),
    };
    // The first frame demands the icon; packing places it for the next request.
    presentation.gui_models.trim_icons.begin_frame();
    let _ = presentation.trim_icon("fixture:coat", 0, &trim, None);
    presentation.finish_trim_art();
    let icon = presentation
        .trim_icon("fixture:coat", 0, &trim, None)
        .expect("inventory icons draw the custom trim");
    let page = &presentation.gui_models.trim_icons.pages()[usize::from(icon.page)
        - presentation.textures.dynamic_start()
        - render_model::UI_TRIM_PAGE_OFFSET];
    let [x, y, _, _] = icon.uv;
    let at = (usize::from(y) * page.dimensions()[0] as usize + usize::from(x)) * 4;
    assert_eq!(&page.pixels()[at..at + 8], &[1, 2, 3, 255, 40, 50, 60, 255]);

    // An icon mask (`icon_trim`) is not resolved yet, so the plain icon draws.
    let masked = custom_armor_presentation(Some(protocol::CustomArmorTrim {
        icon_mask: Some("fixture_coat_trim".into()),
        ..art
    }));
    assert_eq!(custom_world_texels(&masked, "gold"), None);
}
