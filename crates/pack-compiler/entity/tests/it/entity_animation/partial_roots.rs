use super::*;
use serde_json::json;

/// Adds a broken root beside an independently runnable clip or controller.
fn partial_player(root: &str, broken: &str, weighted: bool) -> TempDir {
    let pack = animation_pack(false);
    let path = pack.path().join("entity/test.entity.json");
    let mut entity: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let description = &mut entity["minecraft:client_entity"]["description"];
    description["identifier"] = json!("minecraft:player");
    description["animations"]["optional"] = json!(broken);
    let optional = if weighted {
        json!({"optional": "query.not_a_vanilla_query"})
    } else {
        json!("optional")
    };
    description["scripts"]["animate"] = json!([root, optional]);
    fs::write(path, serde_json::to_vec(&entity).unwrap()).unwrap();
    pack
}

#[test]
fn missing_player_animation_roots_do_not_freeze_valid_clips_or_controllers() {
    for root in ["walk", "main"] {
        for (broken, weighted) in [
            ("animation.fixture.missing", false),
            ("controller.animation.fixture.missing", false),
            ("animation.test.attack", true),
        ] {
            let pack = partial_player(root, broken, weighted);
            let result = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
            let runtime =
                assets::RuntimeEntityAssets::decode(&encode_entity_blob(&result.assets).unwrap())
                    .unwrap();
            let rig = &runtime.rig_bindings()[0];
            assert_eq!(
                rig.fallback,
                EntityRigFallback::Skip,
                "{root} must remain evaluable alongside {broken}, weighted={weighted}"
            );
            let geometry = &runtime.rig_geometries()[rig.first_geometry as usize];
            assert_eq!(geometry.animation_count, u16::from(root == "walk"));
            assert_eq!(geometry.controller_count, u16::from(root == "main"));
            assert!(result.reference_outcomes.iter().any(|outcome| matches!(outcome,
                pack_entity_compiler::CompileReferenceOutcome::OptionalStaticFallback { symbol, .. }
                    if *symbol == rig.entity_symbol
            )), "the rejected optional root must remain observable");
        }
    }
}

#[test]
fn player_with_only_missing_roots_retains_geometry_fallback() {
    let pack = partial_player("optional", "animation.fixture.missing", false);
    let result = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        result.rig_bindings[0].fallback,
        EntityRigFallback::GeometryOnly
    );
    assert!(result.rig_animations.is_empty());
    assert!(result.rig_controllers.is_empty());
}
