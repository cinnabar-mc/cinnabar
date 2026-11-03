//! Worn trim compositions retain the selected mesh and retire unused pixels.
use super::*;

#[test]
fn worn_trim_pixels_are_shared_until_the_appearance_retires() {
    let (fixture, _, mut item) = block_fixture();
    item.identifier = "minecraft:diamond_chestplate".into();
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
                    "textures/trims/color_palettes/trim_palette",
                    vec![100, 100, 100, 255],
                    1,
                ),
                texture(
                    "textures/trims/color_palettes/gold",
                    vec![240, 170, 20, 255],
                    1,
                ),
            ],
        )
        .unwrap(),
    );
    let (mut runtime, pages, _) = EquipmentRuntime::build(
        Arc::clone(&fixture.assets),
        Some(catalog),
        fixture.icons,
        None,
        None,
        ActorArtworkPages::default(),
    );
    let (pages, locations) = pages.with_equipment_rasters(&[EquipmentRaster {
        width: 2,
        height: 1,
        rgba8: vec![10, 20, 30, 255, 40, 50, 60, 255].into(),
    }]);
    let location = locations[0].unwrap();
    let rig = EntityRigId(99);
    let (first, selected) = runtime
        .trim_raster(&item, &pages, location, rig, 0, true)
        .unwrap();
    assert_eq!(selected, rig);
    assert_eq!(first.rgba8.as_ref(), &[240, 170, 20, 255, 40, 50, 60, 255]);
    runtime.begin_shield_frame();
    let (again, _) = runtime
        .trim_raster(&item, &pages, location, rig, 0, true)
        .unwrap();
    assert!(Arc::ptr_eq(&first.rgba8, &again.rgba8));
    runtime.finish_shield_frame();
    runtime.begin_shield_frame();
    runtime.finish_shield_frame();
    let (fresh, _) = runtime
        .trim_raster(&item, &pages, location, rig, 0, true)
        .unwrap();
    assert!(!Arc::ptr_eq(&first.rgba8, &fresh.rgba8));
    item.trim.as_mut().unwrap().material = "unknown".into();
    assert!(
        runtime
            .trim_raster(&item, &pages, location, rig, 0, true)
            .is_none()
    );
}
