//! Shields retain their authored model rather than extruding their GUI thumbnail.
use super::*;

/// Installs an original shield model and texture through normal GUI source preparation.
fn shield_fixture() -> (UiPresentationRuntime, WorldStream) {
    let (_, stream) = fixture_item(assets::gui_item::SHIELD_IDENTIFIER, 0, 0);
    let files = [
        (
            "attachables/shield.json",
            serde_json::json!({"format_version":"1.10.0","minecraft:attachable":{"description":{"identifier":assets::gui_item::SHIELD_IDENTIFIER,"materials":{"default":"entity_alphatest"},"textures":{"default":"textures/fixture_shield"},"geometry":{"default":"geometry.fixture_shield"},"render_controllers":["controller.render.fixture_shield"]}}}),
        ),
        (
            "models/shield.json",
            serde_json::json!({"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.fixture_shield","texture_width":32,"texture_height":32},"bones":[{"name":assets::gui_item::SHIELD_ROOT_BONE,"cubes":[{"origin":[-4,20,-1],"size":[8,8,1],"uv":[0,0]}]}]}]}),
        ),
        (
            "render_controllers/shield.json",
            serde_json::json!({"format_version":"1.8.0","render_controllers":{"controller.render.fixture_shield":{"geometry":"Geometry.default","textures":["Texture.default"],"materials":[{"*":"Material.default"}]}}}),
        ),
    ];
    let compiled = pack_entity_compiler::compile_entity_pack(
        files
            .map(|(path, json)| (path.into(), serde_json::to_vec(&json).unwrap()))
            .to_vec(),
    )
    .unwrap()
    .unwrap();
    let entities = assets::RuntimeEntityAssets::from_compiled(compiled.assets).unwrap();
    let equipment = assets::RuntimeEquipmentCatalog::from_parts(
        [1; 32],
        compiled.equipment_bindings.into_vec(),
        vec![
            assets::EquipmentTexture {
                identifier: "textures/fixture_shield".into(),
                width: 32,
                height: 32,
                rgba8: [10, 20, 30, 255].repeat(1024).into(),
            },
            assets::EquipmentTexture {
                identifier: assets::item::shield::pattern_path("").into(),
                width: 32,
                height: 32,
                rgba8: [255; 4].repeat(1024).into(),
            },
        ],
    )
    .unwrap();
    let catalog = assets::RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog(
            entities.source_manifest_sha256(),
            &[assets::IconSprite {
                width: 16,
                height: 16,
                rgba8: [10, 20, 30, 255].repeat(256).into(),
            }],
            &[assets::IconEntry {
                identifier: assets::gui_item::SHIELD_IDENTIFIER.into(),
                metadata: 0,
                sprite: 0,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    let mut icons = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        client_ui::test_support::fixture_hud(),
        Arc::new(catalog),
    )
    .unwrap();
    icons.set_equipment_catalog(Some(Arc::new(equipment)));
    icons
        .set_gui_models(&RuntimeAssets::diagnostic(), &entities)
        .unwrap();
    (icons, stream)
}

#[test]
fn world_shield_cache_releases_departed_pack_owners() {
    let (mut icons, _) = shield_fixture();
    for admitted in [true, false] {
        let pack = Arc::new(
            assets::RuntimeEquipmentCatalog::from_parts(
                [2; 32],
                vec![],
                vec![assets::EquipmentTexture {
                    identifier: "textures/fixture_shield".into(),
                    width: 32,
                    height: 32,
                    rgba8: [95; 4].repeat(1024).into(),
                }],
            )
            .unwrap(),
        );
        let weak = Arc::downgrade(&pack);
        icons.set_preview_pack_equipment(Some(pack));
        let artwork = icons.world_shield_artwork(None).unwrap();
        let mut cache = ModelCache::default();
        if !admitted {
            cache.budget = render_model::dropped_item::ItemModelBudget::new(
                assets::texture::policy::TexturePolicy::for_device(32),
                1,
                u64::MAX,
            );
        }
        cache.begin_frame();
        cache.finish_demand();
        assert_eq!(
            cache
                .model(
                    ModelKey::Shield(artwork.clone()),
                    || artwork.cost(),
                    || artwork.model()
                )
                .is_some(),
            admitted
        );
        icons.set_preview_pack_equipment(None);
        drop(artwork);
        cache.finish_frame();
        assert!(weak.upgrade().is_some());
        cache.begin_frame();
        cache.finish_frame();
        assert!(
            weak.upgrade().is_none(),
            "absent shield consumers must release both resident and refused pack sources"
        );
    }
}

#[test]
fn patterned_world_shields_follow_pack_replacement_and_draw_one_copy() {
    let (mut icons, stream) = shield_fixture();
    let mut views = Vec::new();
    stream.authority().dropped_items_into(0.0, &mut views);
    let view = &mut views[0];
    view.copy_count = client_world::MAX_DROPPED_ITEM_COPIES as u8;
    view.copy_offsets.fill([0.1, 0.2, 0.3]);
    view.item.shield_pattern = Some(Arc::new(render_api::ShieldPattern {
        base: 15,
        layers: Arc::from([]),
    }));
    let mut cache = ModelCache::default();
    let mut previous = None;
    for color in [None, Some([255, 0, 0, 255]), None] {
        icons.set_preview_pack_equipment(color.map(|color| {
            Arc::new(
                assets::RuntimeEquipmentCatalog::from_parts(
                    [2; 32],
                    vec![],
                    vec![assets::EquipmentTexture {
                        identifier: assets::item::shield::pattern_path("").into(),
                        width: 32,
                        height: 32,
                        rgba8: color.repeat(1024).into(),
                    }],
                )
                .unwrap(),
            )
        }));
        cache.begin_frame();
        cache.finish_demand();
        let preparation = DropPreparation {
            view,
            native_origin: view.position,
            camera: None,
            frustum: None,
        };
        let prepared =
            DroppedItemPublisher::dropped_model(&mut cache, &icons, None, None, None, &preparation)
                .unwrap();
        let mut instances = Vec::new();
        prepared.append(view, &mut instances, |_| (15, 15));
        assert_eq!(instances.len(), 1);
        let instance = instances[0];
        assert_eq!(instance.world_from_item[0][3], view.position[0]);
        assert_eq!(instance.world_from_item[2][3], view.position[2]);
        let DroppedItemModel::Block(model) = &cache.models[instance.model as usize] else {
            panic!("patterned shield requires its model");
        };
        let white = assets::banner::color_rgb(15);
        let expected = color.map_or([white[0], white[1], white[2], 255], |color| {
            [
                white[0] * (color[0] / 255),
                white[1] * (color[1] / 255),
                white[2] * (color[2] / 255),
                255,
            ]
        });
        assert_eq!(&model.materials[0].0.rgba8[..4], &expected);
        if let Some(previous) = previous {
            assert_ne!(instance.model, previous);
        }
        previous = Some(instance.model);
        let artwork = icons
            .world_shield_artwork(view.item.shield_pattern.as_ref())
            .unwrap();
        assert_eq!(
            artwork.cost(),
            render_model::dropped_item::ItemModelCost::from_model(
                &cache.models[instance.model as usize]
            )
        );
    }
}

#[test]
fn dropped_and_framed_shields_keep_the_authored_model() {
    let (icons, stream) = shield_fixture();
    let mut views = Vec::new();
    stream.authority().dropped_items_into(0.0, &mut views);
    assert_eq!(views.len(), 1);
    let preparation = DropPreparation {
        view: &views[0],
        native_origin: views[0].position,
        camera: None,
        frustum: None,
    };
    let mut cache = ModelCache::default();
    cache.begin_frame();
    cache.finish_demand();
    let prepared =
        DroppedItemPublisher::dropped_model(&mut cache, &icons, None, None, None, &preparation)
            .unwrap();
    let mut instances = Vec::new();
    prepared.append(&views[0], &mut instances, |_| (15, 15));
    let DroppedItemModel::Block(model) = &cache.models[instances[0].model as usize] else {
        panic!("dropped shields must keep their authored shield geometry");
    };
    assert_eq!(model.quads.len(), 6);
    assert_eq!(
        model.materials[0].0.rgba8.as_ref(),
        [10, 20, 30, 255].repeat(1024)
    );
    assert_eq!(instances[0].world_from_item[1][1], 0.25);
    let placement = render_model::StaticItemPlacement {
        identifier: assets::gui_item::SHIELD_IDENTIFIER.into(),
        metadata: 0,
        shield_pattern: None,
        potted_block: None,
        carried_block: None,
        allow_block_models: true,
        world_from_item: render_model::item_frame_item_transform([0; 3], 2, 0.0),
        light: None,
    };
    let model = DroppedItemPublisher::placement_model(
        &mut cache,
        &icons,
        None,
        &placement,
        icons.startup_item_icon(&placement.identifier, 0),
    )
    .unwrap();
    assert!(
        matches!(&cache.models[model as usize], DroppedItemModel::Block(model) if model.quads.len() == 6)
    );
}

#[test]
fn refused_world_shield_retains_patterns_and_plain_icon_fallbacks() {
    for base in [None, Some(1), Some(4)] {
        let (mut icons, stream) = shield_fixture();
        icons.set_preview_pack_equipment(Some(Arc::new(
            assets::RuntimeEquipmentCatalog::from_parts(
                [3; 32],
                vec![],
                vec![assets::EquipmentTexture {
                    identifier: "textures/fixture_shield".into(),
                    width: 2048,
                    height: 2048,
                    rgba8: [255; 4].repeat(2048 * 2048).into(),
                }],
            )
            .unwrap(),
        )));
        let mut views = Vec::new();
        stream.authority().dropped_items_into(0.0, &mut views);
        views[0].item.shield_pattern = base.map(|base| {
            Arc::new(render_api::ShieldPattern {
                base,
                layers: Arc::from([]),
            })
        });
        let mut cache = ModelCache::default();
        cache.begin_frame();
        cache.finish_demand();
        for page in 0..4 {
            cache
                .model(
                    ModelKey::icon(
                        ui::IconRef {
                            page,
                            uv: [0, 0, 16, 16],
                            glint: false,
                            color_mask: None,
                        },
                        false,
                    ),
                    || render_model::dropped_item::ItemModelCost::new(16, 1, 1024, 36),
                    || {
                        Some(DroppedItemModel::Sprite(render_model::DroppedItemSprite {
                            width: 16,
                            height: 16,
                            rgba8: vec![255; 1024].into(),
                        }))
                    },
                )
                .unwrap();
        }
        let preparation = DropPreparation {
            view: &views[0],
            native_origin: views[0].position,
            camera: None,
            frustum: None,
        };
        let prepared =
            DroppedItemPublisher::dropped_model(&mut cache, &icons, None, None, None, &preparation)
                .expect("refused shield must retain its fallback");
        let mut instances = Vec::new();
        prepared.append(&views[0], &mut instances, |_| (15, 15));
        let dropped = instances[0].model;
        match (&cache.models[dropped as usize], base) {
            (DroppedItemModel::Block(model), Some(base)) => {
                assert_eq!(instances.len(), 1);
                assert_eq!(model.quads.len(), 6);
                let texture = &model.materials[0].0;
                assert!(texture.width < 2048 && texture.height < 2048);
                let rgb = assets::banner::color_rgb(i64::from(base));
                assert!(
                    texture
                        .rgba8
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|pixel| *pixel == [rgb[0], rgb[1], rgb[2], 255])
                );
                let reduced = icons
                    .world_shield_artwork(views[0].item.shield_pattern.as_ref())
                    .unwrap()
                    .reduced()
                    .unwrap();
                assert_eq!(
                    reduced.cost(),
                    render_model::dropped_item::ItemModelCost::from_model(
                        &cache.models[dropped as usize]
                    )
                );
            }
            (DroppedItemModel::NativeSprite(_) | DroppedItemModel::Sprite(_), None) => {}
            _ => panic!("refused decorated shields must retain their pattern and authored shape"),
        }
        let placement = render_model::StaticItemPlacement {
            identifier: assets::gui_item::SHIELD_IDENTIFIER.into(),
            metadata: 0,
            shield_pattern: views[0].item.shield_pattern.clone(),
            potted_block: None,
            carried_block: None,
            allow_block_models: true,
            world_from_item: render_model::item_frame_item_transform([0; 3], 2, 0.0),
            light: None,
        };
        let framed = DroppedItemPublisher::placement_model(
            &mut cache,
            &icons,
            None,
            &placement,
            icons.startup_item_icon(&placement.identifier, 0),
        )
        .expect("framed shields must also recover from full-sheet refusal");
        if base.is_some() {
            assert_eq!(
                framed, dropped,
                "both world routes must share the recovered decorated model"
            );
        }
    }
}
