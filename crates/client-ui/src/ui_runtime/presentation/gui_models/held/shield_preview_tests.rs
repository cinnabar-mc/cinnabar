//! Decorated shields retain stack artwork when previews need ordinary sprites.
use super::*;
use crate::ui_runtime::presentation::{SessionIcon, SessionIcons, session_icons};

/// Builds original shield artwork with optional authored poses and a session sprite override.
pub(super) fn presentation(
    startup_authored: bool,
    pack_authored: bool,
    session: bool,
) -> UiPresentationRuntime {
    let item = assets::gui_item::SHIELD_IDENTIFIER;
    let pack = fixture(item, "fixture:other_shield");
    let source = pack.equipment.as_ref().unwrap();
    let mut textures = source.textures().to_vec();
    for code in ["", "sc"] {
        textures.push(assets::EquipmentTexture {
            identifier: assets::item::shield::pattern_path(code).into(),
            width: 32,
            height: 32,
            rgba8: [255; 4].repeat(32 * 32).into(),
        });
    }
    let catalog = |authored: bool| {
        let mut bindings = source.bindings().to_vec();
        if !authored {
            for binding in &mut bindings {
                binding.third_person = assets::EquipmentTransform::NeedsMeasurement;
                binding.poses = Box::new([]);
            }
        }
        Arc::new(RuntimeEquipmentCatalog::from_parts([12; 32], bindings, textures.clone()).unwrap())
    };
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
    ui.set_equipment_catalog(Some(catalog(startup_authored)));
    ui.set_gui_models(&assets::RuntimeAssets::diagnostic(), &pack.assets)
        .unwrap();
    ui.set_player_preview_skin(Some(&[255; 64 * 64 * 4]), Default::default());
    ui.set_preview_equipment_assets(
        Some(Arc::clone(&pack.assets)),
        Some(Arc::clone(&pack.assets)),
    );
    ui.set_preview_pack_equipment(Some(catalog(pack_authored)));
    if session {
        session_icons::observe(
            &mut ui,
            Some(&Arc::new(SessionIcons {
                icons: vec![SessionIcon {
                    identifier: item.into(),
                    metadata: 0,
                    width: 16,
                    height: 16,
                    rgba8: [20, 60, 100, 255].repeat(16 * 16).into(),
                }],
                ..Default::default()
            })),
        );
    }
    ui
}

/// Dresses both hands with distinct metadata, optional decoration, upright grips and glint.
fn dress(ui: &mut UiPresentationRuntime, patterns: Option<&[Arc<render_api::ShieldPattern>; 2]>) {
    let item = assets::gui_item::SHIELD_IDENTIFIER;
    let mut player = player_state::PlayerState::new(1);
    player.inventory.set_local_selected_slot(0);
    for (metadata, window) in [
        protocol::PLAYER_INVENTORY_WINDOW_ID,
        protocol::OFFHAND_WINDOW_ID,
    ]
    .into_iter()
    .enumerate()
    {
        player
            .inventory
            .ledger_mut()
            .apply(&protocol::InventoryEvent::Content(
                protocol::InventoryContentEvent {
                    container: protocol::ContainerIdentity::window(window),
                    slots: Arc::from([protocol::NetworkItemStack {
                        network_id: 1,
                        count: 1,
                        metadata: metadata as u32,
                        ..Default::default()
                    }]),
                    storage_item: Default::default(),
                },
            ));
    }
    let runtime = crate::ui_runtime::UiRuntime::new(1);
    ui.dress_player_preview(&player, &runtime, None, Default::default(), |stack| {
        Some(client_world::CanonicalItemStack {
            shield_pattern: patterns.map(|patterns| Arc::clone(&patterns[stack.metadata as usize])),
            grip: assets::item::grip::ItemGrip::Session {
                upright: true,
                mirrored_art: false,
            },
            glint: assets::item::glint::ItemGlint::Session { enabled: true },
            ..crate::test_support::fixture_item(item)
        })
    });
}

#[test]
fn shield_preview_sprite_fallbacks_preserve_each_hands_pattern_and_cached_edges() {
    let item = assets::gui_item::SHIELD_IDENTIFIER;
    for session in [false, true] {
        for (startup_authored, pack_authored, custom_models) in [
            (true, true, false),
            (false, false, false),
            (false, false, true),
            (true, false, true),
        ] {
            let mut ui = presentation(startup_authored, pack_authored, session);
            ui.set_player_preview_custom_item_models(custom_models);
            let patterns = [1, 4].map(|color| {
                Arc::new(render_api::ShieldPattern {
                    base: 15,
                    layers: Arc::from([render_api::BannerPattern {
                        pattern: "sc".into(),
                        color,
                    }]),
                })
            });
            dress(&mut ui, Some(&patterns));
            assert!(
                ui.gui_models
                    .selected_held
                    .iter()
                    .all(|selected| selected.is_some())
            );
            if pack_authored {
                assert!(
                    ui.gui_models
                        .selected_held
                        .iter()
                        .flatten()
                        .all(|selected| selected.model.is_some())
                );
            }
            let models = ui.preview_held_models([true; 2]);
            let repeated = ui.preview_held_models([true; 2]);
            for index in 0..2 {
                let model = models[index].as_ref().unwrap();
                let icon = ui.shield_icon(&patterns[index]).unwrap();
                assert_eq!(model.source, icon.with_glint(true));
                assert!(model.placements.iter().all(|placement| matches!(
                    placement,
                    PreviewHeldPlacement::Sprite {
                        hand_equipped: true
                    }
                )));
                let pixels = ui
                    .resolved_item_sprite(icon, assets::MAX_ICON_SIDE)
                    .unwrap();
                let expected = held_sprite_vertices(
                    pixels.width as usize,
                    pixels.height as usize,
                    &pixels.rgba8,
                    [0.0, 0.0, 1.0, 1.0],
                )
                .unwrap();
                assert_eq!(&*model.vertices, &*expected);
                assert!(Arc::ptr_eq(
                    &model.vertices,
                    &repeated[index].as_ref().unwrap().vertices
                ));
                if index == 0 {
                    let raster = ui.player_preview_gear.held.as_ref().unwrap();
                    assert_eq!(
                        [u32::from(raster.width), u32::from(raster.height)],
                        [pixels.width, pixels.height]
                    );
                    assert_eq!(&*raster.rgba, &*pixels.rgba8);
                }
            }
            assert_ne!(
                models[0].as_ref().unwrap().source,
                models[1].as_ref().unwrap().source
            );
            let first = ui.player_preview_gear.held.clone().unwrap();
            dress(&mut ui, None);
            assert_ne!(first, ui.player_preview_gear.held.clone().unwrap());
            let plain = ui.preview_held_models([true; 2]);
            for model in plain {
                let model = model.expect("plain shields retain their ordinary fallback");
                assert!(model.source.glint);
                if startup_authored && custom_models {
                    assert!(model.placements.iter().all(|placement| matches!(
                        placement,
                        PreviewHeldPlacement::Authored { .. }
                    )));
                } else {
                    assert_eq!(
                        model.source,
                        ui.item_icon(item, 0).unwrap().with_glint(true)
                    );
                    assert!(model.placements.iter().all(|placement| matches!(
                        placement,
                        PreviewHeldPlacement::Sprite {
                            hand_equipped: true
                        }
                    )));
                }
            }
        }
    }
}

#[test]
fn plain_shield_previews_choose_authored_models_or_cached_sprite_fallbacks() {
    let item = assets::gui_item::SHIELD_IDENTIFIER;
    let pack = fixture(item, "fixture:other_shield");
    for startup_authored in [false, true] {
        for pack_authored in [false, true] {
            for custom_models in [false, true] {
                for session in [false, true] {
                    let mut ui = presentation(startup_authored, pack_authored, session);
                    ui.set_player_preview_custom_item_models(custom_models);
                    dress(&mut ui, None);
                    let models = ui.preview_held_models([true; 2]);
                    dress(&mut ui, None);
                    let repeated = ui.preview_held_models([true; 2]);
                    for hand in 0..2 {
                        let model = models[hand].as_ref().unwrap_or_else(|| {
                            panic!("plain hand {hand}: startup={startup_authored}, pack={pack_authored}, custom={custom_models}, session={session}")
                        });
                        assert!(model.source.glint);
                        if (pack_authored || startup_authored) && custom_models {
                            assert_model(&ui, &pack, hand, "ordinary");
                            assert!(model.placements.iter().all(|placement| matches!(
                                placement,
                                PreviewHeldPlacement::Authored { .. }
                            )));
                        } else {
                            let icon = ui.item_icon(item, 0).unwrap();
                            assert_eq!(model.source, icon.with_glint(true));
                            assert!(model.placements.iter().all(|placement| matches!(
                                placement,
                                PreviewHeldPlacement::Sprite {
                                    hand_equipped: true
                                }
                            )));
                            let pixels = ui
                                .resolved_item_sprite(icon, assets::MAX_ICON_SIDE)
                                .unwrap();
                            let expected = held_sprite_vertices(
                                pixels.width as usize,
                                pixels.height as usize,
                                &pixels.rgba8,
                                [0.0, 0.0, 1.0, 1.0],
                            )
                            .unwrap();
                            assert_eq!(&*model.vertices, &*expected);
                        }
                        assert!(Arc::ptr_eq(
                            &model.vertices,
                            &repeated[hand].as_ref().unwrap().vertices
                        ));
                    }
                }
            }
        }
    }
}
