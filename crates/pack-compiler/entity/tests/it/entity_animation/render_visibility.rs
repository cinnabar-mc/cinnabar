use super::*;

#[test]
fn hide_held_items_survives_carrier_roundtrip() {
    for expression in [
        serde_json::json!(true),
        serde_json::json!(false),
        serde_json::json!(1),
        serde_json::json!("query.is_local_player && query.mark_variant == 37 && !query.is_in_ui"),
    ] {
        let pack = animation_pack(false);
        let path = pack.path().join("entity/test.entity.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["minecraft:client_entity"]["description"]["hide_held_items"] = expression;
        let compiled = compile_entity_assets_after_write(pack.path(), &path, &value);
        let encoded = serde_json::to_value(compiled.rig_bindings[0]).unwrap();
        assert!(
            encoded["hide_held_items"].is_u64(),
            "authored visibility retained"
        );
        let runtime =
            assets::RuntimeEntityAssets::decode(&encode_entity_blob(&compiled).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(runtime.rig_bindings()[0]).unwrap(),
            encoded
        );
    }
}

#[test]
fn hide_held_items_omitted_or_malformed_preserves_the_rig() {
    let pack = animation_pack(false);
    let path = pack.path().join("entity/test.entity.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        compile_entity_assets(pack.path(), MANIFEST)
            .unwrap()
            .rig_bindings[0]
            .hide_held_items,
        None
    );
    for expression in [serde_json::json!({}), serde_json::json!("@invalid")] {
        value["minecraft:client_entity"]["description"]["hide_held_items"] = expression;
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let result = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
        assert_eq!(result.assets.rig_bindings[0].hide_held_items, None);
        assert!(result.reference_outcomes.iter().any(|outcome| matches!(
            outcome,
            pack_entity_compiler::CompileReferenceOutcome::OptionalStaticFallback { .. }
        )));
    }
}
