use assets::EquipmentTexture;
use assets::gui_item::SHIELD_ROOT_BONE as ROOT_BONE;

/// Supplies an original constant-color raster.
fn texture(alpha: u8) -> EquipmentTexture {
    EquipmentTexture {
        identifier: "textures/entity/fixture".into(),
        width: 32,
        height: 32,
        rgba8: [200, 60, 20, alpha].repeat(32 * 32).into(),
    }
}

#[test]
fn compiled_shield_defaults_and_explicit_overrides_are_rasterized_once() {
    let scratch = tempfile::tempdir().unwrap();
    for family in [
        "entity",
        "models/entity",
        "animations",
        "animation_controllers",
        "render_controllers",
        "textures/entity",
    ] {
        std::fs::create_dir_all(scratch.path().join(family)).unwrap();
    }
    let mut source = serde_json::json!({"format_version":"1.8.0"});
    for (name, override_cube) in [
        ("geometry.default_shield", false),
        ("geometry.override_shield", true),
    ] {
        let mut cube = serde_json::json!({"origin":[-4,16,-2],"size":[10,18,2],"uv":[0,0]});
        if override_cube {
            cube["inflate"] = 0.0.into();
            cube["mirror"] = false.into();
        }
        source[name] = serde_json::json!({
            "texturewidth":32, "textureheight":32,
            "bones":[{"name":ROOT_BONE,"mirror":true,"inflate":0.75,"cubes":[cube]}]
        });
    }
    std::fs::write(
        scratch.path().join("models/entity/shields.geo.json"),
        source.to_string(),
    )
    .unwrap();
    let compiled = crate::compile_entity_assets(
        scratch.path(),
        include_bytes!("../../../../../../assets/vanilla-source.json"),
    )
    .unwrap();
    assert_eq!(compiled.geometries.len(), 2);
    let mut checker = texture(255);
    checker.rgba8 = (0u8..32)
        .flat_map(|y| (0u8..32).flat_map(move |x| [x * 7, y * 7, (x ^ y) * 7, 255]))
        .collect::<Vec<_>>()
        .into();
    for geometry in &compiled.geometries {
        let overridden = geometry.identifier.as_ref() == "geometry.override_shield";
        assert_eq!(geometry.bones[0].cubes[0].mirror, !overridden);
        assert_eq!(
            geometry.bones[0].cubes[0].inflate.get(),
            if overridden { 0.0 } else { 0.75 }
        );
        let mut resolved_only = geometry.clone();
        resolved_only.bones[0].mirror = None;
        resolved_only.bones[0].inflate = None;
        assert_eq!(
            assets::item::shield::bake_icon(geometry, &checker).unwrap(),
            assets::item::shield::bake_icon(&resolved_only, &checker).unwrap(),
            "{}",
            geometry.identifier
        );
    }
}
