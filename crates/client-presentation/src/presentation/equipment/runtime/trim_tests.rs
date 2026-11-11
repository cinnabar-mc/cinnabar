//! Worn trim compositions retain the selected mesh and retire unused pixels.
use super::*;

/// Supplies original armor and trim pixels without installed game carriers.
fn trim_fixture() -> (EquipmentRuntime, ActorArtworkPages, WornItem) {
    trim_fixture_for("minecraft:diamond_chestplate")
}

/// [`trim_fixture`] for a chestplate definition bound to `identifier`.
fn trim_fixture_for(identifier: &str) -> (EquipmentRuntime, ActorArtworkPages, WornItem) {
    let (fixture, _, mut item) = block_fixture();
    item.identifier = identifier.into();
    item.kind = HeldKind::Sprite;
    item.trim = Some(render_api::ItemTrim {
        material: "gold".into(),
        pattern: "sentry".into(),
    });
    let reference = |identifier: &str| assets::EquipmentReference {
        identifier: identifier.into(),
        resolution: assets::EntityDependencyResolution::External,
    };
    let base_path = "textures/models/armor/diamond_1";
    let binding = assets::EquipmentBinding {
        identifier: item.identifier.as_ref().into(),
        attachable: None,
        category: assets::EquipmentCategory::Armor {
            slot: assets::ArmorSlot::Chestplate,
        },
        geometry: reference("geometry.player.armor.chestplate"),
        texture: reference(base_path),
        resources: Default::default(),
        material: "armor".into(),
        color_mask: assets::EquipmentColorMask::NoMask,
        render_controller: "controller.render.armor".into(),
        first_person: assets::EquipmentTransform::NeedsMeasurement,
        third_person: assets::EquipmentTransform::NeedsMeasurement,
        dropped: assets::EquipmentTransform::NeedsMeasurement,
        poses: Box::new([]),
    };
    let texture = |identifier: &str, rgba8: Vec<u8>, width| assets::EquipmentTexture {
        identifier: identifier.into(),
        width,
        height: 1,
        rgba8: rgba8.into(),
    };
    let catalog = Arc::new(
        RuntimeEquipmentCatalog::from_parts(
            [7; 32],
            vec![binding],
            vec![
                texture(base_path, vec![10, 20, 30, 255, 40, 50, 60, 255], 2),
                texture(
                    "textures/trims/sentry",
                    vec![100, 100, 100, 255, 0, 0, 0, 0],
                    2,
                ),
                texture(
                    "textures/trims/items/chestplate_trim",
                    vec![100, 100, 100, 255, 0, 0, 0, 0],
                    2,
                ),
                texture(
                    "textures/trims/color_palettes/trim_palette",
                    vec![100, 100, 100, 255],
                    1,
                ),
                texture(
                    "textures/trims/color_palettes/gold",
                    vec![240, 170, 20, 255],
                    1,
                ),
                texture(
                    "textures/trims/color_palettes/diamond",
                    vec![30, 250, 250, 255],
                    1,
                ),
                texture(
                    "textures/trims/color_palettes/diamond_darker",
                    vec![10, 100, 100, 255],
                    1,
                ),
                texture(
                    "textures/trims/color_palettes/fixture_icon",
                    vec![1, 2, 3, 255],
                    1,
                ),
            ],
        )
        .unwrap(),
    );
    let (runtime, pages, _) = EquipmentRuntime::build(
        Arc::clone(&fixture.assets),
        Some(catalog),
        fixture.icons,
        None,
        None,
        ActorArtworkPages::default(),
    );
    (runtime, pages, item)
}

#[test]
fn worn_trim_pixels_are_shared_until_the_appearance_retires() {
    let (mut runtime, pages, mut item) = trim_fixture();
    let (pages, locations) = pages.with_equipment_rasters(&[EquipmentRaster {
        width: 2,
        height: 1,
        rgba8: vec![10, 20, 30, 255, 40, 50, 60, 255].into(),
    }]);
    let location = locations[0].unwrap();
    let rig = EntityRigId(99);
    let identity = block_fixture().1.input.identity;
    let catalog = Arc::clone(runtime.catalog.as_ref().unwrap());
    let texture = catalog
        .textures()
        .iter()
        .position(|texture| texture.identifier.as_ref() == "textures/models/armor/diamond_1")
        .unwrap();
    let remember = |runtime: &mut EquipmentRuntime| {
        runtime.trims.remember_worn(
            identity,
            trims::WornSource {
                catalog: Arc::clone(&catalog),
                binding: 0,
                artwork: Arc::clone(&catalog),
                texture,
            },
        );
    };
    remember(&mut runtime);
    let (first, selected) = runtime
        .trim_raster(&item, &pages, location, rig, None, Some(identity))
        .unwrap();
    assert_eq!(selected, rig);
    assert_eq!(first.rgba8.as_ref(), &[240, 170, 20, 255, 40, 50, 60, 255]);
    runtime.begin_shield_frame();
    remember(&mut runtime);
    let (again, _) = runtime
        .trim_raster(&item, &pages, location, rig, None, Some(identity))
        .unwrap();
    assert!(Arc::ptr_eq(&first.rgba8, &again.rgba8));
    runtime.finish_shield_frame();
    runtime.begin_shield_frame();
    runtime.finish_shield_frame();
    remember(&mut runtime);
    let (fresh, _) = runtime
        .trim_raster(&item, &pages, location, rig, None, Some(identity))
        .unwrap();
    assert!(!Arc::ptr_eq(&first.rgba8, &fresh.rgba8));
    item.trim.as_mut().unwrap().material = "unknown".into();
    assert!(
        runtime
            .trim_raster(&item, &pages, location, rig, None, Some(identity))
            .is_none()
    );
}

#[test]
fn trim_dye_uses_the_draw_mask_and_preserves_authored_alpha() {
    use assets::EntityRenderMaterialState;
    use render_api::render_state::{BlendState, RenderState};

    let (mut runtime, pages, item) = trim_fixture();
    let (pages, locations) = pages.with_equipment_rasters(&[EquipmentRaster {
        width: 2,
        height: 1,
        rgba8: vec![10, 20, 30, 64, 40, 50, 60, 64].into(),
    }]);
    let body = block_fixture().1;
    let catalog = Arc::clone(runtime.catalog.as_ref().unwrap());
    let texture = catalog
        .textures()
        .iter()
        .position(|texture| texture.identifier.as_ref() == "textures/models/armor/diamond_1")
        .unwrap();
    runtime.trims.remember_worn(
        body.input.identity,
        trims::WornSource {
            catalog: Arc::clone(&catalog),
            binding: 0,
            artwork: catalog,
            texture,
        },
    );
    let plain = EntityRenderMaterialState::default();
    let masked = EntityRenderMaterialState {
        color_mask: true,
        ..plain
    };
    let emissive = EntityRenderMaterialState {
        emissive: true,
        ..plain
    };
    for (color_mask, state, blend, expected) in [
        (false, None, None, [40, 50, 60, 64]),
        (true, None, None, [35, 37, 44, 255]),
        (true, Some(plain), None, [35, 37, 44, 255]),
        (true, Some(plain), Some(BlendState::ALPHA), [35, 37, 44, 64]),
        (true, Some(emissive), None, [35, 37, 44, 64]),
        (false, Some(masked), None, [35, 37, 44, 64]),
    ] {
        let mut draw = body.clone();
        draw.material = render_model::ActorMaterial {
            color_mask,
            state,
            render_state: Some(RenderState {
                blend,
                ..assets::entity::material::UNAUTHORED_RENDER_STATE
            }),
            ..Default::default()
        };
        draw.tint = u32::from_le_bytes([128, 0, 0, 255]);
        let (raster, _) = runtime
            .trim_draw(&item, &pages, locations[0].unwrap(), &mut draw, true)
            .unwrap();
        assert_eq!(&raster.rgba8[..4], &[240, 170, 20, 255]);
        assert_eq!(&raster.rgba8[4..], &expected);
        assert_eq!(
            draw.tint,
            if color_mask || state.is_some_and(|state| state.color_mask) {
                0
            } else {
                u32::from_le_bytes([128, 0, 0, 255])
            }
        );
    }
}

#[test]
fn worn_trim_palette_follows_the_item_when_a_pack_renames_its_texture() {
    let (mut runtime, pages, mut item) = trim_fixture();
    item.trim.as_mut().unwrap().material = "diamond".into();
    let pixels: Arc<[u8]> = Arc::from([10, 20, 30, 255, 40, 50, 60, 255]);
    let selected_artwork = Arc::new(
        RuntimeEquipmentCatalog::from_parts(
            [8; 32],
            Vec::new(),
            vec![assets::EquipmentTexture {
                identifier: "textures/custom/coat".into(),
                width: 2,
                height: 1,
                rgba8: Arc::clone(&pixels),
            }],
        )
        .unwrap(),
    );
    let (pages, locations) = pages.with_equipment_rasters(&[EquipmentRaster {
        width: 2,
        height: 1,
        rgba8: pixels,
    }]);
    let identity = block_fixture().1.input.identity;
    runtime.trims.remember_worn(
        identity,
        trims::WornSource {
            catalog: Arc::clone(runtime.catalog.as_ref().unwrap()),
            binding: 0,
            artwork: selected_artwork,
            texture: 0,
        },
    );
    let (raster, _) = runtime
        .trim_raster(
            &item,
            &pages,
            locations[0].unwrap(),
            EntityRigId(99),
            None,
            Some(identity),
        )
        .unwrap();
    assert_eq!(raster.rgba8.as_ref(), &[10, 100, 100, 255, 40, 50, 60, 255]);
}

#[test]
fn held_trim_decorates_the_selected_session_icon() {
    use client_ui::ui_runtime::presentation::{SessionIcon, SessionIcons};
    let (mut runtime, pages, item) = trim_fixture();
    let icons = SessionIcons {
        icons: vec![SessionIcon {
            identifier: Arc::clone(&item.identifier),
            metadata: item.metadata,
            width: 2,
            height: 1,
            rgba8: vec![10, 20, 30, 255, 40, 50, 60, 255].into(),
        }],
        ..Default::default()
    };
    let staged = StagedSessionIcons::stage_icons(&icons).unwrap();
    let (pages, locations) = pages.with_equipment_rasters(staged.rasters());
    runtime.set_session_items(None, Some(staged), locations);
    let (rig, location, _) = runtime.held_icon_mesh(&item).unwrap();
    let (_, placement, _) = runtime
        .session_sprite(&item.identifier, item.metadata)
        .unwrap();
    let (decorated, selected) = runtime
        .trim_raster(&item, &pages, location, rig, None, None)
        .expect("the admitted session icon must receive the stack trim");
    assert_eq!(selected, rig);
    let at =
        (usize::from(placement.y) * usize::from(decorated.width) + usize::from(placement.x)) * 4;
    assert_eq!(
        &decorated.rgba8[at..at + 8],
        &[240, 170, 20, 255, 40, 50, 60, 255]
    );
}

#[test]
fn worn_trim_selection_reuses_slots_and_requires_the_current_draw() {
    let (runtime, _, _) = trim_fixture();
    let catalog = Arc::clone(runtime.catalog.as_ref().unwrap());
    let source = trims::WornSource {
        catalog: Arc::clone(&catalog),
        binding: 0,
        artwork: catalog,
        texture: 0,
    };
    let mut identity = block_fixture().1.input.identity;
    let mut cache = trims::Trims::default();
    cache.remember_worn(identity, source.clone());
    let (_, allocations) = test_alloc::count(|| {
        for revision in 2..130 {
            let previous = identity;
            identity.movement_revision = revision;
            cache.begin_frame();
            assert!(cache.worn_source(previous).is_none());
            cache.remember_worn(identity, source.clone());
            assert!(cache.worn_source(identity).is_some());
            assert!(cache.worn_source(previous).is_none());
            cache.finish_frame();
        }
    });
    assert_eq!(
        allocations, 0,
        "unchanged visible armor must reuse its source slot"
    );
    cache.begin_frame();
    cache.finish_frame();
    assert!(cache.worn_source(identity).is_none());
}

/// Declares `identifier` as server chestplate armor tagged `minecraft:trimmable_armors`, with an
/// icon palette for `icon_palette` materials.
fn custom_armor_items(
    identifier: &Arc<str>,
    icon_palette: &[&str],
    icons: Option<Arc<client_ui::ui_runtime::presentation::SessionIcons>>,
) -> crate::session_assets::SessionItems {
    let art = protocol::CustomArmorTrim {
        icon_mask: None,
        icon_palettes: icon_palette
            .iter()
            .map(|material| {
                (
                    Arc::from(*material),
                    Arc::from("textures/trims/color_palettes/fixture_icon"),
                )
            })
            .collect(),
    };
    crate::session_assets::SessionItems {
        components: Arc::new(
            [(
                Arc::clone(identifier),
                protocol::ItemComponents {
                    icon: Some("fixture_coat".into()),
                    wearable_slot: Some("slot.armor.chest".into()),
                    armor_trim: Some(Arc::new(art)),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        ),
        icons,
    }
}

#[test]
fn worn_trimmable_custom_armor_draws_its_trim_without_a_darker_palette() {
    let (mut runtime, pages, mut item) = trim_fixture_for("fixture:coat");
    item.trim.as_mut().unwrap().material = "diamond".into();
    let (pages, locations) = pages.with_equipment_rasters(&[EquipmentRaster {
        width: 2,
        height: 1,
        rgba8: vec![10, 20, 30, 255, 40, 50, 60, 255].into(),
    }]);
    let identity = block_fixture().1.input.identity;
    let catalog = Arc::clone(runtime.catalog.as_ref().unwrap());
    let remember = |runtime: &mut EquipmentRuntime| {
        runtime.trims.remember_worn(
            identity,
            trims::WornSource {
                catalog: Arc::clone(&catalog),
                binding: 0,
                artwork: Arc::clone(&catalog),
                texture: 0,
            },
        );
    };
    let draw = |runtime: &mut EquipmentRuntime| {
        runtime.trim_raster(
            &item,
            &pages,
            locations[0].unwrap(),
            EntityRigId(99),
            None,
            Some(identity),
        )
    };
    remember(&mut runtime);
    assert!(
        draw(&mut runtime).is_none(),
        "untagged custom armor takes no trim"
    );
    // The icon's palette never reaches worn armor, which reads its attachable's aliases.
    runtime.set_session_items(
        Some(&custom_armor_items(&item.identifier, &["diamond"], None)),
        None,
        Vec::new(),
    );
    remember(&mut runtime);
    let (raster, _) = draw(&mut runtime).expect("trimmable custom armor draws its trim");
    assert_eq!(raster.rgba8.as_ref(), &[30, 250, 250, 255, 40, 50, 60, 255]);
}

#[test]
fn held_trimmable_custom_armor_icon_uses_its_icon_palette() {
    use client_ui::ui_runtime::presentation::{SessionIcon, SessionIcons};
    let (mut runtime, pages, item) = trim_fixture_for("fixture:coat");
    let icons = Arc::new(SessionIcons {
        icons: vec![SessionIcon {
            identifier: Arc::clone(&item.identifier),
            metadata: item.metadata,
            width: 2,
            height: 1,
            rgba8: vec![10, 20, 30, 255, 40, 50, 60, 255].into(),
        }],
        ..Default::default()
    });
    let staged = StagedSessionIcons::stage_icons(&icons).unwrap();
    let (pages, locations) = pages.with_equipment_rasters(staged.rasters());
    runtime.set_session_items(
        Some(&custom_armor_items(
            &item.identifier,
            &["gold"],
            Some(icons),
        )),
        Some(staged),
        locations,
    );
    let (rig, location, _) = runtime.held_icon_mesh(&item).unwrap();
    let (_, placement, _) = runtime
        .session_sprite(&item.identifier, item.metadata)
        .unwrap();
    let (decorated, _) = runtime
        .trim_raster(&item, &pages, location, rig, None, None)
        .expect("the custom icon must receive the stack trim");
    let at =
        (usize::from(placement.y) * usize::from(decorated.width) + usize::from(placement.x)) * 4;
    assert_eq!(
        &decorated.rgba8[at..at + 8],
        &[1, 2, 3, 255, 40, 50, 60, 255]
    );
}
