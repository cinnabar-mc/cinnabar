//! Shield compositions follow the admitted stack, selected base sheet and active pack.
use super::*;

/// Supplies original mask pixels without any installed game carriers.
fn catalog(mask: [u8; 4]) -> Arc<RuntimeEquipmentCatalog> {
    Arc::new(
        RuntimeEquipmentCatalog::from_parts(
            [7; 32],
            Vec::new(),
            vec![assets::EquipmentTexture {
                identifier: assets::item::shield::pattern_path("").into(),
                width: 2,
                height: 1,
                rgba8: [mask, [0; 4]].concat().into(),
            }],
        )
        .unwrap(),
    )
}

#[test]
fn shield_decoration_follows_selected_artwork_and_recovers_after_pack_reload() {
    let (fixture, _, mut item) = block_fixture();
    let (mut runtime, pages, _) = EquipmentRuntime::build(
        Arc::clone(&fixture.assets),
        Some(catalog([255; 4])),
        fixture.icons,
        None,
        None,
        ActorArtworkPages::default(),
    );
    let source = render_model::EquipmentRaster {
        width: 2,
        height: 1,
        rgba8: [10, 20, 30, 255, 40, 50, 60, 255].into(),
    };
    let (pages, locations) = pages.with_equipment_rasters(&[source]);
    let location = locations[0].unwrap();
    item.identifier = assets::gui_item::SHIELD_IDENTIFIER.into();
    assert!(
        runtime
            .shield_raster(&item, &pages, location, EntityRigId(99))
            .is_none()
    );
    item.shield_pattern = Some(Arc::new(render_api::ShieldPattern {
        base: 15,
        layers: Arc::from([]),
    }));
    runtime.begin_shield_frame();
    let first = runtime
        .shield_raster(&item, &pages, location, EntityRigId(99))
        .unwrap();
    assert_eq!(&first.rgba8[..3], assets::banner::color_rgb(15));
    assert_eq!(&first.rgba8[4..], &[40, 50, 60, 255]);
    let reused = runtime
        .shield_raster(&item, &pages, location, EntityRigId(99))
        .unwrap();
    assert!(Arc::ptr_eq(&first.rgba8, &reused.rgba8));
    runtime.finish_shield_frame();
    runtime.begin_shield_frame();
    let next = runtime
        .shield_raster(&item, &pages, location, EntityRigId(99))
        .unwrap();
    assert!(Arc::ptr_eq(&first.rgba8, &next.rgba8));
    runtime.finish_shield_frame();
    runtime.set_pack_layer(Some((
        Arc::clone(&runtime.assets),
        Some(catalog([255, 0, 0, 255])),
        Vec::new(),
    )));
    let changed = runtime
        .shield_raster(&item, &pages, location, EntityRigId(99))
        .unwrap();
    assert_eq!(
        &changed.rgba8[..3],
        &[assets::banner::color_rgb(15)[0], 0, 0]
    );
    assert!(!Arc::ptr_eq(&first.rgba8, &changed.rgba8));
    runtime.begin_shield_frame();
    runtime.finish_shield_frame();
    let recovered = runtime
        .shield_raster(&item, &pages, location, EntityRigId(99))
        .unwrap();
    assert!(!Arc::ptr_eq(&changed.rgba8, &recovered.rgba8));
    assert_eq!(changed.rgba8, recovered.rgba8);
}

/// Builds an original board, its UV sheet, and a full-sheet white cloth mask.
fn sprite_sources() -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = vec![
        ("attachables/shield.json".into(), serde_json::to_vec(&serde_json::json!({
            "format_version":"1.10.0", "minecraft:attachable":{"description":{
                "identifier":assets::gui_item::SHIELD_IDENTIFIER,
                "materials":{"default":"entity_alphatest"},
                "geometry":{"default":"geometry.fixture.shield"},
                "textures":{"default":"textures/fixture/shield"},
                "render_controllers":["controller.render.fixture.shield"]
            }}
        })).unwrap()),
        ("models/shield.json".into(), br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.fixture.shield","texture_width":32,"texture_height":32},"bones":[{"name":"shield","cubes":[{"origin":[-4,16,-2],"size":[10,18,2],"uv":[0,0]}]}]}]}"#.to_vec()),
        ("render_controllers/shield.json".into(), br#"{"format_version":"1.8.0","render_controllers":{"controller.render.fixture.shield":{"geometry":"Geometry.default","textures":["Texture.default"],"materials":[{"*":"Material.default"}]}}}"#.to_vec()),
    ];
    for (path, color) in [
        ("textures/fixture/shield", [40, 50, 60, 255]),
        (assets::item::shield::pattern_path("").as_str(), [255; 4]),
    ] {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(32, 32, image::Rgba(color))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        files.push((format!("{path}.png").into(), bytes.into_inner()));
    }
    files
}

#[test]
fn shield_sprite_fallback_decorates_only_its_projected_icon_rectangle() {
    use client_ui::ui_runtime::presentation::{SessionIcon, SessionIcons};
    let (mut runtime, mut pages) =
        crate::presentation::equipment::tests::startup_runtime(sprite_sources());
    let (_, _, mut item) = block_fixture();
    item.identifier = assets::gui_item::SHIELD_IDENTIFIER.into();
    item.shield_pattern = Some(Arc::new(render_api::ShieldPattern {
        base: 1,
        layers: Arc::from([]),
    }));
    // The first icon forces the shield away from the atlas origin.
    let icons = SessionIcons {
        icons: ["fixture:other", assets::gui_item::SHIELD_IDENTIFIER]
            .map(|identifier| SessionIcon {
                identifier: identifier.into(),
                metadata: 0,
                width: 64,
                height: 64,
                rgba8: [40, 50, 60, 255].repeat(64 * 64).into(),
            })
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let staged = StagedSessionIcons::stage_icons(&icons).unwrap();
    let (next, locations) = pages.with_equipment_rasters(staged.rasters());
    pages = next;
    runtime.set_session_items(None, Some(staged), locations);
    let (rig, location, _) = runtime.held_icon_mesh(&item).unwrap();
    let (_, placement, _) = runtime.session_sprite(&item.identifier, 0).unwrap();
    assert!(placement.x > 0);
    let output = runtime.shield_raster(&item, &pages, location, rig).unwrap();
    assert_eq!(
        &output.rgba8[..4],
        &[40, 50, 60, 255],
        "unrelated atlas icons retain their artwork"
    );
    let catalog = runtime.catalog.as_ref().unwrap();
    let geometry = &runtime.assets.geometries()[0];
    let base = catalog.texture("textures/fixture/shield").unwrap();
    let composed =
        assets::item::shield::compose(base, 1, [], 32 * 32, |path| catalog.texture(path)).unwrap();
    let expected = assets::item::shield::bake_icon(geometry, &composed).unwrap();
    for y in 0..64usize {
        let start = ((usize::from(placement.y) + y) * usize::from(output.width)
            + usize::from(placement.x))
            * 4;
        assert_eq!(
            &output.rgba8[start..start + 64 * 4],
            &expected.rgba8[y * 64 * 4..(y + 1) * 64 * 4]
        );
    }
}
