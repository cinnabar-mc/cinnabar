use super::material_states::{per_bone_pack, summary, visible};
use super::{MANIFEST, write};
use serde_json::{Value, json};

/// Compiles the per-bone fixture with `bindings` as its Cinnabar extension document and returns
/// each fixture bone's material summary.
fn bone_materials(bindings: Option<Value>) -> Vec<(&'static str, Vec<Value>)> {
    let fixture = per_bone_pack(json!([{"*":"Material.default"}]), false);
    if let Some(bindings) = bindings {
        write(
            fixture.path(),
            pack_entity_compiler::MATERIAL_BINDINGS_PATH,
            &serde_json::to_vec(&bindings).unwrap(),
        );
    }
    let compiled = pack_entity_compiler::compile_entity_assets(fixture.path(), MANIFEST).unwrap();
    ["magnifyingGlass", "handle"]
        .into_iter()
        .map(|bone| {
            let summaries = compiled
                .render
                .layers
                .iter()
                .filter(|layer| visible(&compiled, layer, bone))
                .map(|layer| summary(&compiled.render, layer))
                .collect();
            (bone, summaries)
        })
        .collect()
}

fn glowing_glass(binding: Value) -> Value {
    json!({
        "format_version": "1.0.0",
        "materials": {
            "example:glow": {
                "base": "entity_alphatest",
                "material": {"+states": ["Blending"], "+defines": ["USE_EMISSIVE"],
                             "blendSrc": "One", "blendDst": "One"},
                "textures": {"glow_mask": "textures/entity/example_glow"},
                "parameters": {"strength": {"scalar": 2.0}, "tint": {"color": [1.0, 0.5, 0.0, 1.0]}}
            }
        },
        "bindings": [binding]
    })
}

#[test]
fn a_bone_binding_draws_only_matching_bones_with_the_extension_material() {
    let cutout = json!({"alpha_test":true,"cull":false,"blend":false,"depth_write":true});
    let glow = json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":true,
        "emissive":true,"additive":true});
    let bones = bone_materials(Some(glowing_glass(json!({
        "entity": "minecraft:example", "bone": "magnifying*", "material": "example:glow"
    }))));
    assert_eq!(bones[0], ("magnifyingGlass", vec![glow]));
    assert_eq!(bones[1], ("handle", vec![cutout]));
}

#[test]
fn a_binding_without_a_bone_replaces_the_whole_controller_material() {
    let bones = bone_materials(Some(glowing_glass(json!({
        "entity": "minecraft:example",
        "render_controller": "controller.render.ex*",
        "material": "example:glow"
    }))));
    assert!(
        bones
            .iter()
            .all(|(_, summaries)| summaries[0]["blend"] == true)
    );
}

#[test]
fn bindings_for_other_targets_or_invalid_documents_leave_bedrock_materials_unchanged() {
    let unchanged = bone_materials(None);
    for document in [
        glowing_glass(json!({"entity": "minecraft:other", "material": "example:glow"})),
        glowing_glass(json!({
            "entity": "minecraft:example",
            "render_controller": "controller.render.unrelated",
            "material": "example:glow"
        })),
        glowing_glass(json!({"entity": "minecraft:example", "material": "example:missing"})),
        json!({"format_version": "2.0.0"}),
        json!({"format_version": "1.0.0", "unknown": true}),
    ] {
        assert_eq!(bone_materials(Some(document)), unchanged);
    }
}
