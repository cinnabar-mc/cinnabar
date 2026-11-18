use super::*;

#[test]
fn inherited_skeleton_metadata_preserves_defaults_without_copying_cubes() {
    let model = assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.fixture"}}"#,
        r#"{"format_version":"1.12.0","minecraft:geometry":[{
            "description":{"identifier":"geometry.fixture"},
            "bones":[{"name":"head","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]}]
        }]}"#,
    )
    .unwrap()
    .unwrap();
    let mut base = bone_metadata(&model.bones[0]);
    assert!(base.cubes.is_empty());
    assert!(base.texture_meshes.is_empty());
    let mut child = model.bones[0].clone();
    child.pivot = Some([1.0, 2.0, 3.0].map(|v| assets::EntityGeometryScalar::new(v).unwrap()));
    child.rotation = Some([4.0, 5.0, 6.0].map(|v| assets::EntityGeometryScalar::new(v).unwrap()));
    child.binding = Some("query.fixture".into());
    overlay_bone(&mut base, &child);
    assert_eq!(base.pivot, child.pivot);
    assert_eq!(base.rotation, child.rotation);
    assert_eq!(base.binding, child.binding);
    assert!(base.cubes.is_empty());
    child.reset = Some(true);
    overlay_bone(&mut base, &child);
    assert!(base.cubes.is_empty());
    child.cubes = Box::default();
    overlay_bone(&mut base, &child);
    assert!(base.cubes.is_empty());
    let (bones, names) = skeleton(&[base]).unwrap();
    assert_eq!(names[0].as_ref(), "head");
    assert_eq!(bones[0].pivot, [-1.0, 2.0, 3.0]);
    assert_eq!(bones[0].rotation, [4.0, 5.0, 6.0]);
    assert!(bones[0].has_binding_expression);
}

#[test]
fn merged_bone_indices_follow_the_inherited_skeleton_order() {
    let mut compiled = crate::test_support::compiled_fixture();
    let parent = compiled.geometries[0].clone();
    let bone = |name: &str| assets::EntityGeometryBone {
        name: name.into(),
        ..parent.bones[0].clone()
    };
    let mut child = parent.clone();
    child.identifier = "geometry.layer".into();
    child.inherits = Some(assets::EntityGeometryInheritance {
        identifier: parent.identifier.clone(),
        resolution: assets::EntityDependencyResolution::Catalog,
    });
    child.bones = vec![bone("wing"), bone("RightItem"), bone("tail"), bone("WING")].into();
    let mut symbols = compiled.symbols.into_vec();
    symbols.insert(
        1,
        assets::EntityAssetSymbol {
            identifier: child.identifier.clone(),
            ..symbols[0].clone()
        },
    );
    compiled.symbols = symbols.into();
    // Symbols after the inserted geometry move up by one.
    compiled.animation_clips[0].symbol += 1;
    compiled.rig_bindings[0].entity_symbol += 1;
    compiled.rig_bindings[0].render_controller = compiled.rig_bindings[0]
        .render_controller
        .map(|index| index + 1);
    compiled.geometries = vec![parent, child].into();
    let assets = RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap();
    let (_, names) = resolve_bones(&assets, 1).unwrap();
    assert_eq!(names.len(), 3);
    for (index, name) in names.iter().enumerate() {
        assert_eq!(merged_bone_index(&assets, 1, name), Some(index));
    }
    assert_eq!(merged_bone_index(&assets, 1, "RIGHTITEM"), Some(0));
    assert_eq!(merged_bone_index(&assets, 1, "absent"), None);
}
