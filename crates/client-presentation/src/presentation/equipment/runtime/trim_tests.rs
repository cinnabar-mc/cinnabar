//! Worn trim compositions retain the selected mesh and retire unused pixels.
use super::*;

/// Supplies original armor and trim pixels without installed game carriers.
fn trim_fixture() -> (EquipmentRuntime, ActorArtworkPages, WornItem) {
    let (fixture, _, mut item) = block_fixture();
    item.identifier = "minecraft:diamond_chestplate".into();
    item.kind = HeldKind::Sprite;
    item.trim = Some(protocol::ItemTrim {
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
