//! Selected hand sheets and inventory thumbnails share one recoverable shield budget.
use super::*;

#[test]
fn shield_atlas_pressure_fits_owner_selected_sheets_with_inventory_art() {
    let item = assets::gui_item::SHIELD_IDENTIFIER;
    let pack = fixture(item, "fixture:other_shield");
    let source = pack.equipment.as_ref().unwrap();
    let side = render_model::UI_MODEL_ATLAS_SIDE as u16;
    let mut textures = source.textures().to_vec();
    for texture in &mut textures {
        texture.width = side;
        texture.height = side;
        texture.rgba8 = texture.rgba8[..4].repeat(usize::from(side).pow(2)).into();
    }
    // Only half the sheet is cloth, so different selected base sheets stay distinct.
    let mut mask = vec![0; usize::from(side).pow(2) * 4];
    mask[..usize::from(side).pow(2) * 2].fill(255);
    textures.push(assets::EquipmentTexture {
        identifier: assets::item::shield::pattern_path("").into(),
        width: side,
        height: side,
        rgba8: mask.into(),
    });
    let catalog = Arc::new(
        RuntimeEquipmentCatalog::from_parts([11; 32], source.bindings().to_vec(), textures)
            .unwrap(),
    );
    let icons = Arc::new(
        RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog(
                pack.assets.source_manifest_sha256(),
                &[assets::IconSprite {
                    width: 16,
                    height: 16,
                    rgba8: vec![255; 1024].into(),
                }],
                &[assets::IconEntry {
                    identifier: item.into(),
                    metadata: 0,
                    sprite: 0,
                }],
            )
            .unwrap(),
        )
        .unwrap(),
    );
    let mut ui = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        icons,
    )
    .unwrap();
    ui.set_equipment_catalog(Some(Arc::clone(&catalog)));
    ui.set_gui_models(&assets::RuntimeAssets::diagnostic(), &pack.assets)
        .unwrap();
    ui.set_player_preview_skin(Some(&[255; 64 * 64 * 4]), Default::default());
    ui.set_preview_equipment_assets(Some(Arc::clone(&pack.assets)), None);
    ui.set_player_preview_gear([None; 4], Some((item, 0)));
    let appearances: Vec<_> = (0..4)
        .map(|base| {
            Arc::new(render_api::ShieldPattern {
                base,
                layers: Arc::from([]),
            })
        })
        .collect();
    ui.prepare_shield_icons(appearances.iter().cloned());
    assert_eq!(
        ui.gui_models.shield_icons.pages().len(),
        render_model::MAX_UI_SHIELD_PAGES
    );
    let hand = player_preview::PreviewHandItem {
        identifier: item.into(),
        metadata: 0,
        shield_pattern: Some(appearances[0].clone()),
        grip: None,
        glint: Some(false),
        use_animation: assets::item_use::ItemUseAnimation::None,
        charged_projectile: assets::item_use::ItemProjectileRole::None,
    };
    let world = owner(Arc::clone(&pack.assets));
    let context = PreviewEquipmentOwner {
        actor: world.authority().actor(1).unwrap(),
        rig: world.authority().actor_rig(1).unwrap(),
        input: Default::default(),
    };
    let hands = [Some((hand, false)), None];
    ui.prepare_preview_hands(&hands, Some(context), [None; 4]);
    let selected = ui.preview_held_models([true; 2])[0].clone().unwrap().source;
    let selected_sheet = catalog.texture("textures/mapped").unwrap();
    assert_eq!(
        selected,
        ui.shield_texture(&appearances[0], selected_sheet)
            .expect("the selected shield sheet must fit with inventory artwork")
    );
    assert!(
        selected.uv[2] - selected.uv[0] < side,
        "the complete demand must retry at a smaller resolution"
    );
    let icons: Vec<_> = appearances
        .iter()
        .map(|appearance| ui.shield_icon(appearance).unwrap())
        .collect();
    ui.publish_shield_icons();
    let pages = ui.gui_models.shield_icons.pages();
    assert!(pages.len() <= render_model::MAX_UI_SHIELD_PAGES);
    assert!(
        appearances
            .iter()
            .all(|appearance| ui.shield_icon(appearance).is_some())
    );
    ui.prepare_shield_icons(appearances.iter().cloned());
    ui.prepare_preview_hands(&hands, Some(context), [None; 4]);
    assert_eq!(
        selected,
        ui.preview_held_models([true; 2])[0].clone().unwrap().source
    );
    assert_eq!(
        icons,
        appearances
            .iter()
            .map(|appearance| ui.shield_icon(appearance).unwrap())
            .collect::<Vec<_>>()
    );
    let reused = ui.gui_models.shield_icons.pages();
    assert!(
        pages
            .iter()
            .zip(&reused)
            .all(|(before, after)| std::ptr::eq(before.pixels(), after.pixels())),
        "unchanged demand must retain published pixel allocations"
    );
}
