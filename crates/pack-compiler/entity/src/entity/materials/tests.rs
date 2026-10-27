use assets::entity::material::{
    EntityMaterialBlendFactor as Factor, EntityMaterialCompare, EntityMaterialFilter,
    EntityMaterialState as State,
};
use serde_json::json;

use super::*;

/// Resolves `name` from files read in order and returns its interned definition.
fn definition(files: &[Value], name: &str) -> Option<(EntityMaterialDefinition, MaterialTable)> {
    let mut states = MaterialStates::default();
    for file in files {
        states.read(file);
    }
    let mut table = MaterialTable::default();
    let (_, index) = states.resolve(name, &mut table).unwrap()?;
    let definition = table.definitions[index as usize].clone();
    Some((definition, table))
}

fn modern(materials: Value) -> Value {
    let mut materials = materials;
    materials["version"] = json!("1.0.0");
    json!({ "materials": materials })
}

#[test]
fn modern_files_replace_plain_lists_and_apply_signed_lists_otherwise() {
    let file = modern(json!({
        "base": {"states": ["Blending", "DisableCulling"], "defines": ["A", "B"]},
        "replaced:base": {"states": ["DisableDepthWrite"]},
        "edited:base": {"+states": ["EnableAlphaToCoverage"], "-states": ["Blending"], "-defines": ["A"]}
    }));
    let (replaced, _) = definition(std::slice::from_ref(&file), "replaced").unwrap();
    assert_eq!(replaced.states.as_ref(), [State::DisableDepthWrite]);
    let (edited, _) = definition(&[file], "edited").unwrap();
    assert_eq!(
        edited.states.as_ref(),
        [State::DisableCulling, State::EnableAlphaToCoverage]
    );
    assert!(edited.has_define("B") && !edited.has_define("A"));
    assert!(edited.has_define("ALPHA_TO_COVERAGE"));
}

#[test]
fn legacy_files_only_add_even_when_a_material_claims_a_newer_version() {
    let file = json!({
        "base": {"states": ["Blending"]},
        "child:base": {"version": "1.0.0", "states": ["DisableCulling"], "-states": ["Blending"]}
    });
    let (child, _) = definition(&[file], "child").unwrap();
    assert_eq!(
        child.states.as_ref(),
        [State::Blending, State::DisableCulling]
    );
}

#[test]
fn first_declaration_wins_and_plus_declarations_modify_in_place() {
    let first = modern(json!({"glow:entity": {"+defines": ["FIRST"]}}));
    let second = modern(json!({
        "glow:entity": {"+defines": ["IGNORED"]},
        "+glow": {"+defines": ["MODIFIED"]}
    }));
    let (glow, _) = definition(&[first, second], "glow").unwrap();
    assert!(glow.has_define("FIRST") && glow.has_define("MODIFIED"));
    assert!(!glow.has_define("IGNORED"));
}

#[test]
fn parents_resolve_within_their_file_or_from_native_bases() {
    let parent = modern(json!({"shared": {"+defines": ["SHARED"]}}));
    let child = modern(json!({"child:shared": {}, "native:entity_alphatest": {}}));
    assert!(definition(&[parent, child.clone()], "child").is_none());
    let (native, _) = definition(&[child], "native").unwrap();
    assert!(native.has_define("ALPHA_TEST"));
}

#[test]
fn cross_file_parents_with_a_native_base_resolve_through_the_pack_declaration() {
    let patched = modern(json!({"entity_alphatest": {"+defines": ["PATCHED"]}}));
    let custom = modern(json!({"my:entity_alphatest": {}}));
    let (alone, _) = definition(std::slice::from_ref(&custom), "my").unwrap();
    assert!(alone.has_define("ALPHA_TEST") && !alone.has_define("PATCHED"));
    let (child, _) = definition(&[patched, custom], "my").unwrap();
    assert!(child.has_define("ALPHA_TEST") && child.has_define("PATCHED"));
}

#[test]
fn runtime_fields_follow_native_defaults_and_widths() {
    let file = modern(json!({"fields": {
        "blendSrc": "One",
        "blendDst": "DestColor",
        "stencilRef": 258,
        "depthFunc": "Always",
        "samplerStates": [{"samplerIndex": 2, "textureFilter": "Bilinear", "shaderStage": "Vertex"}, {"samplerIndex": 9}],
        "vertexFields": [{"field": "Position"}, {"field": "UV0"}]
    }}));
    let (fields, _) = definition(&[file], "fields").unwrap();
    assert_eq!(
        fields.blend,
        [
            Factor::One,
            Factor::DestColor,
            Factor::One,
            Factor::DestColor
        ]
    );
    assert_eq!(fields.stencil_ref, Some(2), "the client keeps one byte");
    assert_eq!(fields.depth_func, EntityMaterialCompare::Always);
    assert_eq!(fields.samplers.len(), 1);
    assert_eq!(fields.samplers[0].filter, EntityMaterialFilter::Bilinear);
    assert_eq!(fields.samplers[0].stages, 1);
    assert_eq!(
        fields.vertex_fields.as_ref().map(|fields| fields.len()),
        Some(2)
    );
}

#[test]
fn biases_that_overflow_f32_keep_the_inherited_value() {
    let file = modern(json!({
        "base": {"depthBias": 2.5, "slopeScaledDepthBias": -1.0},
        "child:base": {"depthBias": 1e39, "slopeScaledDepthBias": -1e300}
    }));
    let (child, table) = definition(&[file], "child").unwrap();
    assert_eq!(child.depth_bias, 2.5);
    assert_eq!(child.slope_scaled_depth_bias, -1.0);
    assert!(
        table
            .definitions
            .iter()
            .all(|definition| definition.depth_bias.is_finite()
                && definition.slope_scaled_depth_bias.is_finite())
    );
}

#[test]
fn modern_sampler_lists_reset_and_legacy_lists_overlay() {
    let modern_file = modern(json!({
        "base": {"samplerStates": [{"samplerIndex": 0}, {"samplerIndex": 1}]},
        "child:base": {"samplerStates": [{"samplerIndex": 3}]}
    }));
    let (child, _) = definition(&[modern_file], "child").unwrap();
    assert_eq!(
        child.samplers.iter().map(|s| s.slot).collect::<Vec<_>>(),
        [3]
    );
    let legacy_file = json!({
        "base": {"samplerStates": [{"samplerIndex": 0}]},
        "child:base": {"samplerStates": [{"samplerIndex": 3}]}
    });
    let (child, _) = definition(&[legacy_file], "child").unwrap();
    assert_eq!(
        child.samplers.iter().map(|s| s.slot).collect::<Vec<_>>(),
        [0, 3]
    );
}

#[test]
fn variants_clone_their_owner_and_intern_into_the_table() {
    let file = modern(json!({"owner": {
        "+defines": ["OWNER"],
        "variants": [{"skinned": {"+defines": ["SKIN"]}}, {"plain": {}}]
    }}));
    let (owner, table) = definition(&[file], "owner").unwrap();
    assert_eq!(owner.variants.len(), 2);
    let skinned = &table.definitions[owner.variants[0].definition as usize];
    assert!(skinned.has_define("OWNER") && skinned.has_define("SKIN"));
    assert!(skinned.variants.is_empty());
}

#[test]
fn shading_follows_defines_over_native_bases() {
    let file = modern(json!({"hidden:entity_alphatest": {"-defines": ["USE_OVERLAY"]}}));
    let mut states = MaterialStates::default();
    states.read(&file);
    let mut table = MaterialTable::default();
    let (state, _) = states.resolve("hidden", &mut table).unwrap().unwrap();
    assert!(state.alpha_test && state.disable_overlay && !state.emissive);
}

#[test]
fn native_bases_keep_their_shading_and_raster() {
    let alpha = Some((Factor::SourceAlpha, Factor::OneMinusSrcAlpha));
    for (name, alpha_test, overlay, culls, blending, depth_write) in [
        ("skeleton", true, true, false, None, true),
        ("harness", true, true, false, None, true),
        ("experience_orb", true, true, true, None, true),
        ("slime_outer", false, true, true, alpha, true),
        ("breeze_wind", false, false, false, alpha, false),
        (
            "charged_creeper",
            false,
            false,
            false,
            Some((Factor::One, Factor::One)),
            true,
        ),
        ("entity_emissive_alpha", true, true, false, None, true),
        ("entity_static", false, false, true, None, true),
    ] {
        let mut table = MaterialTable::default();
        let (state, index) = MaterialStates::default()
            .resolve(name, &mut table)
            .unwrap()
            .unwrap();
        assert_eq!(
            (state.alpha_test, !state.disable_overlay),
            (alpha_test, overlay),
            "{name}"
        );
        let definition = &table.definitions[index as usize];
        assert_eq!(
            !definition.has_state(State::DisableCulling),
            culls,
            "{name}"
        );
        assert_eq!(
            definition
                .has_state(State::Blending)
                .then_some((definition.blend[0], definition.blend[1])),
            blending,
            "{name}"
        );
        assert_eq!(
            !definition.has_state(State::DisableDepthWrite),
            depth_write,
            "{name}"
        );
    }
}
