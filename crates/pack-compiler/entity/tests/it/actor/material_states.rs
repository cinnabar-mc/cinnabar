use super::{MANIFEST, pack, write};
use assets::{
    EntityRenderData, EntityRenderLayer,
    entity::material::{
        EntityMaterialBlendFactor as Factor, EntityMaterialCompare, EntityMaterialState as State,
    },
};
use serde_json::{Value, json};

/// Summarizes a layer's shading features and its definition's raster as the flags these
/// fixtures assert: cutout, back-face culling, blending, depth writes, emissive weighting,
/// additive factors, overlay suppression, depth comparisons and channel writes. False optional
/// flags are omitted.
pub(super) fn summary(render: &EntityRenderData, layer: &EntityRenderLayer) -> Value {
    let (Some(state), Some(definition)) = (
        layer.material_state,
        layer
            .material_definition
            .and_then(|index| render.materials.get(index as usize)),
    ) else {
        return Value::Null;
    };
    let blend = definition.has_state(State::Blending);
    let additive = blend && definition.blend[1] == Factor::One;
    let mut summary = json!({
        "alpha_test": state.alpha_test,
        "cull": !definition.has_state(State::DisableCulling),
        "blend": blend,
        "depth_write": !definition.has_state(State::DisableDepthWrite),
    });
    for (key, enabled) in [
        ("emissive", state.emissive),
        ("only_emissive", state.only_emissive),
        ("additive", additive),
        (
            "additive_alpha",
            additive && definition.blend[0] == Factor::SourceAlpha,
        ),
        ("disable_overlay", state.disable_overlay),
        (
            "depth_always",
            definition.depth_func == EntityMaterialCompare::Always,
        ),
        (
            "depth_equal",
            definition.depth_func == EntityMaterialCompare::Equal,
        ),
        (
            "disable_alpha_write",
            definition.has_state(State::DisableAlphaWrite),
        ),
        (
            "disable_color_write",
            definition.has_state(State::DisableColorWrite),
        ),
    ] {
        if enabled {
            summary[key] = true.into();
        }
    }
    summary
}

/// Compiles and round-trips the material selected by one fixture actor.
fn state(material: &str, definitions: Option<&Value>) -> Value {
    let fixture = pack(0, material, false);
    if let Some(definitions) = definitions {
        write(
            fixture.path(),
            "materials/entity.material",
            &serde_json::to_vec(definitions).unwrap(),
        );
    }
    let compiled = pack_entity_compiler::compile_entity_assets(fixture.path(), MANIFEST).unwrap();
    assert_eq!(compiled.render.layers.len(), 1);
    let bytes = assets::encode_entity_blob(&compiled).unwrap();
    let runtime = assets::RuntimeEntityAssets::decode(&bytes).unwrap();
    summary(runtime.render_data(), &runtime.render_data().layers[0])
}

#[test]
fn actor_overlay_define_is_inherited_added_removed_and_replaced() {
    let definitions = json!({"materials": {"version": "1.0.0",
        "fixture_base": {"defines": [], "states": []},
        "fixture_overlay:fixture_base": {"+defines": ["USE_OVERLAY"]},
        "fixture_inherited:fixture_overlay": {},
        "fixture_removed:fixture_overlay": {"-defines": ["USE_OVERLAY"]},
        "fixture_replaced:fixture_overlay": {"defines": ["ALPHA_TEST"]}
    }});
    for material in ["fixture_base", "fixture_removed", "fixture_replaced"] {
        assert_eq!(
            state(material, Some(&definitions))["disable_overlay"],
            true,
            "{material} does not admit the actor overlay"
        );
    }
    for material in ["fixture_overlay", "fixture_inherited"] {
        assert!(
            state(material, Some(&definitions))["disable_overlay"]
                .as_bool()
                .is_none_or(|disabled| !disabled),
            "{material} inherits USE_OVERLAY"
        );
    }
}

#[test]
fn charged_material_keeps_texture_rgb_and_adds_it_without_actor_overlay() {
    let expected = json!({"alpha_test":false,"cull":false,"blend":true,
        "depth_write":true,"additive":true,"disable_overlay":true});
    assert_eq!(state("charged_creeper", None), expected);
    assert_eq!(state("charged_creeper.skinning", None), expected);
    let definitions = json!({"materials": {"version": "1.0.0",
        "fixture_cutout:charged_creeper": {"-states": ["Blending"]},
        "fixture_aura:entity_static": {
            "+defines": ["USE_UV_ANIM", "ALPHA_TEST"],
            "+states": ["Blending", "DisableCulling"],
            "blendSrc": "One", "blendDst": "One"
        }
    }});
    assert_eq!(state("fixture_aura", Some(&definitions)), expected);
    assert_eq!(
        state("fixture_cutout", Some(&definitions)),
        json!({"alpha_test":true,"cull":false,"blend":false,"depth_write":true,
            "disable_overlay":true})
    );
}

#[test]
fn actor_material_states_distinguish_authored_one_sided_and_nocull_alpha_test() {
    assert_eq!(
        state("entity_alphatest_one_sided", None),
        json!({"alpha_test":true,"cull":true,"blend":false,"depth_write":true})
    );
    assert_eq!(
        state("entity_alphatest", None),
        json!({"alpha_test":true,"cull":false,"blend":false,"depth_write":true})
    );
}

#[test]
fn skeleton_alias_uses_double_sided_alpha_test_without_material_definitions() {
    let expected = json!({"alpha_test":true,"cull":false,"blend":false,"depth_write":true});
    assert_eq!(state("skeleton", None), expected);
    assert_eq!(state("skeleton.skinning", None), expected);
}

#[test]
fn spectator_material_blends_and_respects_authored_raster_overrides() {
    assert_eq!(
        state("player_spectator", None),
        json!({"alpha_test":false,"cull":true,"blend":true,"depth_write":true})
    );
    let definitions = json!({"materials": {"version": "1.0.0",
        "player_spectator:entity": {"+states":["DisableDepthWrite"],"+defines":["ALPHA_TEST"]}
    }});
    assert_eq!(
        state("player_spectator", Some(&definitions)),
        json!({"alpha_test":true,"cull":true,"blend":false,"depth_write":false})
    );
}

#[test]
fn emissive_materials_keep_alpha_as_lighting_weight_and_authored_additive_inheritance() {
    assert_eq!(
        state("entity_emissive_alpha", None),
        json!({"alpha_test":true,"cull":false,"blend":false,"depth_write":true,"emissive":true})
    );
    let definitions = json!({"materials":{
        "version":"1.0.0",
        "fixture_reflection:entity_emissive":{
            "+defines":["USE_UV_ANIM","ALPHA_TEST"],
            "+states":["Blending","DisableCulling"],
            "blendSrc":"One","blendDst":"One"
        },
        "fixture_child:fixture_reflection":{"+defines":["USE_OVERLAY"]},
        "fixture_ordinary:fixture_child":{"-defines":["USE_EMISSIVE"],
            "blendSrc":"SourceAlpha","blendDst":"OneMinusSrcAlpha"}
    }});
    let expected = json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":true,
        "emissive":true,"additive":true});
    assert_eq!(state("fixture_reflection", Some(&definitions)), expected);
    assert_eq!(state("fixture_child", Some(&definitions)), expected);
    assert_eq!(
        state("fixture_ordinary", Some(&definitions)),
        json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":true})
    );
}

#[test]
fn additive_material_child_can_weight_source_alpha_without_replacing_destination() {
    let definitions = json!({"materials":{
        "version":"1.0.0",
        "fixture_glow:entity_emissive":{
            "+states":["Blending","DisableCulling","DisableDepthWrite"],
            "blendSrc":"One","blendDst":"One"
        },
        "fixture_faded:fixture_glow":{"blendSrc":"SourceAlpha"},
        "fixture_inherited:fixture_faded":{"+defines":["USE_UV_ANIM"]},
        "fixture_opaque:fixture_faded":{"blendSrc":"One"},
        "fixture_standard:fixture_faded":{"blendDst":"OneMinusSrcAlpha"}
    }});
    let expected = json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":false,
        "emissive":true,"additive":true,"additive_alpha":true});
    assert_eq!(state("fixture_faded", Some(&definitions)), expected);
    assert_eq!(state("fixture_inherited", Some(&definitions)), expected);
    assert_eq!(
        state("fixture_opaque", Some(&definitions)),
        json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":false,
            "emissive":true,"additive":true})
    );
    assert_eq!(
        state("fixture_standard", Some(&definitions)),
        json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":false,"emissive":true})
    );
}

#[test]
fn shader_blending_define_does_not_enable_experience_orb_raster_blending() {
    assert_eq!(
        state("experience_orb", None),
        json!({"alpha_test":true,"cull":true,"blend":false,"depth_write":true})
    );
}

#[test]
fn actor_material_states_keep_slime_outer_blended_culled_and_depth_writing() {
    assert_eq!(
        state("slime_outer", None),
        json!({"alpha_test":false,"cull":true,"blend":true,"depth_write":true})
    );
}

#[test]
fn sulfur_cube_material_aliases_match_authored_parent_states() {
    let definitions = json!({"materials": {"version": "1.0.0",
        "fixture_inner:entity_alphatest": {},
        "fixture_outer:entity": {"+states": ["Blending"]},
        "fixture_override:sulfur_cube_outer": {"-states": ["Blending"]}
    }});
    for (native, authored) in [
        ("sulfur_cube", "fixture_inner"),
        ("sulfur_cube_outer", "fixture_outer"),
    ] {
        let expected = state(authored, Some(&definitions));
        assert!(expected.is_object());
        assert_eq!(state(native, None), expected);
        assert_eq!(state(&format!("{native}.skinning"), None), expected);
    }
    assert_eq!(
        state("fixture_override", Some(&definitions)),
        json!({"alpha_test":false,"cull":true,"blend":false,"depth_write":true})
    );
}

#[test]
fn wind_material_uses_the_native_double_sided_blended_route_without_depth_writes() {
    let expected = json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":false,"disable_overlay":true});
    assert_eq!(state("breeze_wind", None), expected);
    // Native material inheritance still includes ALPHA_TEST, but Blending selects
    // the separate transparent Actor pass after the inherited declaration is read.
    let definitions = json!({"materials": {"version": "1.0.0",
        "version": "1.0.0",
        "fixture_wind:entity_static": {
            "+defines": ["ALPHA_TEST", "USE_UV_ANIM"],
            "+states": ["Blending", "DisableCulling", "DisableDepthWrite"]
        },
        "fixture_child:fixture_wind": {"+defines": ["ALPHA_TEST"]},
        "fixture_cutout:fixture_child": {"-states": ["Blending"]}
    }});
    assert_eq!(state("fixture_child", Some(&definitions)), expected);
    assert_eq!(
        state("fixture_cutout", Some(&definitions)),
        json!({"alpha_test":true,"cull":false,"blend":false,"depth_write":false,"disable_overlay":true})
    );
}

pub(super) fn per_bone_pack(rules: Value, alternate: bool) -> tempfile::TempDir {
    let fixture = pack(0, "entity_alphatest", false);
    write(
        fixture.path(),
        "entity/example.entity.json",
        &serde_json::to_vec(&json!({"format_version":"1.8.0","minecraft:client_entity":{
            "description":{
                "identifier":"minecraft:example",
                "geometry":{"default":"geometry.example","other":"geometry.other"},
                "materials":{"default":"entity_alphatest","glass":"slime_outer"},
                "textures":{"default":"textures/entity/example"},
                "render_controllers":["controller.render.example"]
            }
        }}))
        .unwrap(),
    );
    let bone = |name: &str, parent: Option<&str>| {
        let mut bone = json!({"name":name,"cubes":[{"origin":[0,0,0],"size":[0,2,7],"uv":[0,0]}]});
        if let Some(parent) = parent {
            bone["parent"] = json!(parent);
        }
        bone
    };
    write(
        fixture.path(),
        "models/entity/example.geo.json",
        &serde_json::to_vec(&json!({"format_version":"1.12.0","minecraft:geometry":[
            {"description":{"identifier":"geometry.example","texture_width":16,"texture_height":16},
             "bones":[bone("root",None),bone("magnifyingGlass",Some("root")),bone("handle",Some("magnifyingGlass"))]},
            {"description":{"identifier":"geometry.other","texture_width":16,"texture_height":16},
             "bones":[bone("root",None),bone("otherGlass",Some("root"))]}
        ]})).unwrap(),
    );
    write(
        fixture.path(),
        "render_controllers/example.json",
        &serde_json::to_vec(&json!({"format_version":"1.8.0","render_controllers":{
            "controller.render.example":{
                "geometry":if alternate {"query.is_alive ? Geometry.default : Geometry.other"} else {"Geometry.default"},
                "materials":rules,"textures":["Texture.default"],
                "part_visibility":[{"root":false}]
            }
        }})).unwrap(),
    );
    fixture
}

pub(super) fn visible(
    compiled: &assets::CompiledEntityAssets,
    layer: &assets::EntityRenderLayer,
    name: &str,
) -> bool {
    let mut visible = true;
    let first = layer.first_visibility as usize;
    for rule in &compiled.render.visibility[first..first + usize::from(layer.visibility_count)] {
        if rule.pattern.as_ref() != "*" && !rule.pattern.eq_ignore_ascii_case(name) {
            continue;
        }
        let expression = compiled.molang_expressions[rule.condition as usize];
        let operations = &compiled.molang_ops[expression.first_op as usize
            ..expression.first_op as usize + usize::from(expression.op_count)];
        visible = match operations {
            [assets::MolangOp::Push(value)] => value.get() != 0.0,
            _ => panic!("fixture visibility must be constant"),
        };
    }
    visible
}

#[test]
fn actor_material_rules_partition_bones_without_revealing_hidden_parts_or_hiding_children() {
    let fixture = per_bone_pack(
        json!([
            {"*":"Material.default"},{"magnifyingGlass":"Material.glass"}
        ]),
        false,
    );
    let compiled = pack_entity_compiler::compile_entity_assets(fixture.path(), MANIFEST).unwrap();
    assert_eq!(compiled.render.layers.len(), 2);
    for name in ["root", "magnifyingGlass", "handle"] {
        let states: Vec<_> = compiled
            .render
            .layers
            .iter()
            .filter(|layer| visible(&compiled, layer, name))
            .map(|layer| summary(&compiled.render, layer))
            .collect();
        match name {
            "root" => assert!(states.is_empty(), "authored visibility stays hidden"),
            "magnifyingGlass" => assert_eq!(
                states,
                vec![json!({"alpha_test":false,"cull":true,"blend":true,"depth_write":true})]
            ),
            _ => assert_eq!(
                states,
                vec![json!({"alpha_test":true,"cull":false,"blend":false,"depth_write":true})]
            ),
        }
    }
    let bytes = assets::encode_entity_blob(&compiled).unwrap();
    assets::RuntimeEntityAssets::decode(&bytes).unwrap();
}

#[test]
fn actor_material_rules_use_the_last_match_and_cover_alternate_geometry_bones() {
    let fixture = per_bone_pack(
        json!([
            {"*":"Material.default"},{"magnifyingGlass":"Material.glass"},
            {"otherGlass":"Material.glass"}
        ]),
        true,
    );
    let compiled = pack_entity_compiler::compile_entity_assets(fixture.path(), MANIFEST).unwrap();
    assert_eq!(compiled.render.layers.len(), 2);
    for name in ["magnifyingGlass", "otherGlass"] {
        let layers: Vec<_> = compiled
            .render
            .layers
            .iter()
            .filter(|layer| visible(&compiled, layer, name))
            .collect();
        assert_eq!(layers.len(), 1);
        assert_eq!(summary(&compiled.render, layers[0])["blend"], true);
        assert_eq!(layers[0].geometry_count, 2);
    }
    let fixture = per_bone_pack(
        json!([
            {"*":"Material.default"},{"magnifyingGlass":"Material.glass"},
            {"*":"Material.default"}
        ]),
        false,
    );
    let compiled = pack_entity_compiler::compile_entity_assets(fixture.path(), MANIFEST).unwrap();
    assert_eq!(compiled.render.layers.len(), 1);
    assert_eq!(
        summary(&compiled.render, &compiled.render.layers[0])["blend"],
        false
    );
    assert!(visible(
        &compiled,
        &compiled.render.layers[0],
        "magnifyingGlass"
    ));
}

#[test]
fn actor_material_states_select_inherited_blending_without_losing_depth_write() {
    let definitions = json!({"materials":{
        "version":"1.0.0",
        "fixture_text:entity_alphatest":{"-defines":["FANCY"]},
        "fixture_plate:fixture_text":{"+states":["Blending"]}
    }});
    assert_eq!(
        state("fixture_plate", Some(&definitions)),
        json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":true})
    );
}

#[test]
fn actor_material_states_apply_explicit_depth_and_culling_overrides_independently() {
    let definitions = json!({"materials":{
        "version":"1.0.0",
        "fixture_plate:entity_alphatest":{
            "+states":["Blending","DisableDepthWrite"],
            "-states":["DisableCulling"]
        },
        "fixture_opaque:fixture_plate":{
            "-states":["Blending"],"-defines":["ALPHA_TEST"]
        }
    }});
    assert_eq!(
        state("fixture_plate", Some(&definitions)),
        json!({"alpha_test":false,"cull":true,"blend":true,"depth_write":false})
    );
    assert_eq!(
        state("fixture_opaque", Some(&definitions)),
        json!({"alpha_test":false,"cull":true,"blend":false,"depth_write":false})
    );
}

#[test]
fn actor_material_states_replacement_excludes_add_remove_for_the_same_family() {
    let definitions = json!({"materials":{
        "version":"1.0.0",
        "fixture:entity_alphatest":{
            "states":["DisableDepthWrite"],
            "+states":["Blending"],
            "-states":["DisableDepthWrite"],
            "defines":["ALPHA_TEST"],
            "-defines":["ALPHA_TEST"]
        }
    }});
    assert_eq!(
        state("fixture", Some(&definitions)),
        json!({"alpha_test":true,"cull":true,"blend":false,"depth_write":false,"disable_overlay":true})
    );
}

#[test]
fn always_passing_depth_function_is_inherited_and_restored_by_less_equal() {
    let definitions = json!({"materials":{
        "version":"1.0.0",
        "fixture_marker:entity":{"depthFunc":"Always"},
        "fixture_marker_cutout:fixture_marker":{"+defines":["ALPHA_TEST"]},
        "fixture_marker_tested:fixture_marker":{"depthFunc":"LessEqual"}
    }});
    assert_eq!(
        state("fixture_marker", Some(&definitions)),
        json!({"alpha_test":false,"cull":true,"blend":false,"depth_write":true,
            "depth_always":true})
    );
    assert_eq!(
        state("fixture_marker_cutout", Some(&definitions)),
        json!({"alpha_test":true,"cull":true,"blend":false,"depth_write":true,
            "depth_always":true})
    );
    assert_eq!(
        state("fixture_marker_tested", Some(&definitions)),
        json!({"alpha_test":false,"cull":true,"blend":false,"depth_write":true})
    );
}

#[test]
fn native_emissive_aliases_preserve_authored_body_and_eye_states() {
    for (material, alpha_test, cull, only_emissive) in [
        ("blaze_body", false, true, false),
        ("blaze_head", true, false, false),
        ("magma_cube", true, false, false),
        ("glow_squid", false, true, false),
        ("spider", true, false, false),
        ("spider_invisible", true, false, true),
        ("enderman", true, false, false),
        ("enderman_invisible", true, false, true),
        ("phantom", true, false, false),
        ("phantom_invisible", true, false, true),
    ] {
        for name in [material.to_owned(), format!("{material}.skinning")] {
            let compiled = state(&name, None);
            assert_eq!(compiled["emissive"], true, "{name}");
            assert_eq!(compiled["alpha_test"], alpha_test, "{name}");
            assert_eq!(compiled["cull"], cull, "{name}");
            assert_eq!(compiled["blend"], false, "{name}");
            assert_eq!(compiled["depth_write"], true, "{name}");
            assert_eq!(
                compiled["only_emissive"].as_bool().unwrap_or(false),
                only_emissive,
                "{name}"
            );
        }
    }
}

#[test]
fn only_emissive_define_is_inherited_removed_and_replaced() {
    let definitions = json!({"materials": {"version": "1.0.0",
        "fixture_eyes:spider_invisible": {},
        "fixture_body:fixture_eyes": {"-defines": ["USE_ONLY_EMISSIVE"]},
        "fixture_replaced:fixture_eyes": {"defines": ["ALPHA_TEST"]},
        "spider:entity": {"+defines": ["ALPHA_TEST"]}
    }});
    assert_eq!(
        state("fixture_eyes", Some(&definitions))["only_emissive"],
        true
    );
    for name in ["fixture_body", "fixture_replaced", "spider"] {
        assert_ne!(
            state(name, Some(&definitions))["only_emissive"],
            true,
            "{name}"
        );
    }
    assert_ne!(state("spider", Some(&definitions))["emissive"], true);
}

#[test]
fn generated_materials_reach_the_carrier_without_fallback() {
    for (name, native) in assets::facts::pack::rules().material_states() {
        let mut expected = json!({
            "alpha_test": native.alpha_test && !native.blend,
            "cull": native.cull,
            "blend": native.blend,
            "depth_write": native.depth_write,
        });
        for (key, enabled) in [
            ("emissive", native.emissive),
            ("only_emissive", native.only_emissive),
            ("additive", native.blend && native.additive),
            (
                "additive_alpha",
                native.blend && native.additive && native.additive_alpha,
            ),
            ("disable_overlay", native.disable_overlay),
            ("depth_always", native.depth_always),
            ("depth_equal", native.depth_equal && !native.depth_always),
            ("disable_alpha_write", native.disable_alpha_write),
            ("disable_color_write", native.disable_color_write),
        ] {
            if enabled {
                expected[key] = true.into();
            }
        }
        assert_eq!(state(name, None), expected, "{name}");
    }
    assert!(state("cushion", None).is_null());
}

#[test]
fn ordinary_aliases_keep_cutouts_and_one_sided_limbs() {
    for name in ["allay", "armor_stand", "arrow"] {
        assert_eq!(
            state(name, None),
            json!({"alpha_test":true,"cull":false,"blend":false,"depth_write":true}),
            "{name}"
        );
    }
    assert_eq!(
        state("turtle", None),
        json!({"alpha_test":false,"cull":true,"blend":false,"depth_write":true})
    );
    assert_eq!(
        state("axolotl_limbs", None),
        json!({"alpha_test":true,"cull":true,"blend":false,"depth_write":true})
    );
    assert_eq!(
        state("wither_boss_armor", None),
        json!({"alpha_test":false,"cull":false,"blend":true,"depth_write":true,"additive":true,"disable_overlay":true})
    );
}

#[test]
fn depth_and_channel_write_states_inherit_and_reset_independently() {
    let definitions = json!({"materials": {"version": "1.0.0",
        "fixture:entity": {"depthFunc":"Equal", "+states":["DisableAlphaWrite","DisableColorWrite"]},
        "fixture_child:fixture": {},
        "fixture_removed:fixture": {"depthFunc":"LessEqual", "-states":["DisableAlphaWrite","DisableColorWrite"]},
        "fixture_replaced:fixture": {"depthFunc":"Always", "states":[]}
    }});
    for name in ["fixture", "fixture_child"] {
        assert_eq!(
            state(name, Some(&definitions)),
            json!({"alpha_test":false,"cull":true,"blend":false,"depth_write":true,"depth_equal":true,"disable_alpha_write":true,"disable_color_write":true})
        );
    }
    assert_eq!(
        state("fixture_removed", Some(&definitions)),
        json!({"alpha_test":false,"cull":true,"blend":false,"depth_write":true})
    );
    assert_eq!(
        state("fixture_replaced", Some(&definitions)),
        json!({"alpha_test":false,"cull":true,"blend":false,"depth_write":true,"depth_always":true})
    );
}

/// The value of a compiled condition that folds to a constant.
fn constant(compiled: &assets::CompiledEntityAssets, condition: u32) -> Option<f32> {
    let expression = compiled.molang_expressions[condition as usize];
    match &compiled.molang_ops[expression.first_op as usize
        ..expression.first_op as usize + usize::from(expression.op_count)]
    {
        [assets::MolangOp::Push(value)] => Some(value.get()),
        _ => None,
    }
}

#[test]
fn material_expressions_draw_each_selectable_material_under_its_condition() {
    let materials = |rules: Value| {
        let fixture = per_bone_pack(rules, false);
        pack_entity_compiler::compile_entity_assets(fixture.path(), MANIFEST).unwrap()
    };
    // A queried choice: one layer per material, each drawn only under its own condition, so a
    // pack prewarms both and a switch needs no new pipeline.
    let queried = materials(json!([{"*":"query.is_baby ? Material.glass : Material.default"}]));
    assert_eq!(queried.render.layers.len(), 2);
    let definitions = queried
        .render
        .layers
        .iter()
        .map(|layer| layer.material_definition)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        definitions.len(),
        2,
        "both selectable materials are compiled"
    );
    assert!(
        queried
            .render
            .layers
            .iter()
            .all(|layer| layer.condition.is_some())
    );
    // The selection follows the expression's value.
    for (selector, drawn) in [("1.0", "glass"), ("0.0", "default")] {
        let compiled =
            materials(json!([{ "*": format!("{selector} ? Material.glass : Material.default") }]));
        let active = compiled
            .render
            .layers
            .iter()
            .filter(|layer| {
                layer.condition.is_none_or(|condition| {
                    constant(&compiled, condition).is_some_and(|value| value != 0.0)
                })
            })
            .map(|layer| summary(&compiled.render, layer))
            .collect::<Vec<_>>();
        let expected = materials(json!([{ "*": format!("Material.{drawn}") }]));
        assert_eq!(
            active,
            vec![summary(&expected.render, &expected.render.layers[0])],
            "{selector}"
        );
    }
    // An array selection draws each element under its index.
    let fixture = per_bone_pack(json!([{"*":"Array.looks[query.variant]"}]), false);
    write(
        fixture.path(),
        "render_controllers/example.json",
        &serde_json::to_vec(&json!({"format_version":"1.8.0","render_controllers":{
            "controller.render.example":{
                "arrays":{"materials":{"Array.looks":["Material.default","Material.glass"]}},
                "geometry":"Geometry.default",
                "materials":[{"*":"Array.looks[query.variant]"}],
                "textures":["Texture.default"]
            }
        }}))
        .unwrap(),
    );
    let indexed = pack_entity_compiler::compile_entity_assets(fixture.path(), MANIFEST).unwrap();
    assert_eq!(indexed.render.layers.len(), 2);
    assert!(
        indexed
            .render
            .layers
            .iter()
            .all(|layer| layer.condition.is_some())
    );
}

#[test]
fn controllers_draw_with_at_most_their_first_three_texture_expressions() {
    let fixture = per_bone_pack(json!([{"*":"Material.default"}]), false);
    write(
        fixture.path(),
        "render_controllers/example.json",
        &serde_json::to_vec(&json!({"format_version":"1.8.0","render_controllers":{
            "controller.render.example":{
                "geometry":"Geometry.default",
                "materials":[{"*":"Material.default"}],
                "textures":["Texture.default","Texture.default","Texture.default","Texture.default"]
            }
        }}))
        .unwrap(),
    );
    let compiled = pack_entity_compiler::compile_entity_assets(fixture.path(), MANIFEST).unwrap();
    assert!(!compiled.render.layers.is_empty());
    for layer in compiled.render.layers.iter() {
        assert_eq!(
            layer.slot_count, 3,
            "Bedrock never evaluates a fourth texture"
        );
    }
}
