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
    let (_, index) = states.resolve(name, &mut table)?;
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
fn modern_variant_removals_drop_inherited_variants() {
    let file = modern(json!({
        "base": {"variants": [{"skinned": {}}, {"glint": {}}, {"plain": {}}]},
        "child:base": {"+variants": [{"extra": {}}], "-variants": ["glint", "extra"]}
    }));
    let (child, _) = definition(&[file], "child").unwrap();
    let names: Vec<_> = child
        .variants
        .iter()
        .map(|variant| variant.name.as_ref())
        .collect();
    assert_eq!(names, ["skinned", "plain"]);
    let legacy = json!({
        "base": {"variants": [{"glint": {}}]},
        "child:base": {"version": "1.0.0", "-variants": ["glint"]}
    });
    let (child, _) = definition(&[legacy], "child").unwrap();
    assert_eq!(child.variants.len(), 1, "legacy files never remove");
}

#[test]
fn shading_follows_defines_over_native_bases() {
    let file = modern(json!({"hidden:entity_alphatest": {"-defines": ["USE_OVERLAY"]}}));
    let mut states = MaterialStates::default();
    states.read(&file);
    let mut table = MaterialTable::default();
    let (state, _) = states.resolve("hidden", &mut table).unwrap();
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
        let (state, index) = MaterialStates::default().resolve(name, &mut table).unwrap();
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
    for (name, color_mask, multitexture) in [
        ("sheep", true, false),
        ("wolf", true, false),
        ("llama", false, true),
        ("entity_alphatest", false, false),
    ] {
        let (state, _) = MaterialStates::default()
            .resolve(name, &mut MaterialTable::default())
            .unwrap();
        assert_eq!(
            (state.color_mask, state.multitexture),
            (color_mask, multitexture),
            "{name}"
        );
    }
}

#[test]
fn modifications_of_a_builtin_only_material_apply_to_it_and_its_children() {
    let patch = modern(json!({"+entity_alphatest": {"+defines": ["PATCHED"]}}));
    let child = modern(json!({"my:entity_alphatest": {}}));
    let (base, _) = definition(std::slice::from_ref(&patch), "entity_alphatest").unwrap();
    assert!(base.has_define("ALPHA_TEST") && base.has_define("PATCHED"));
    let (child, _) = definition(&[patch, child], "my").unwrap();
    assert!(child.has_define("ALPHA_TEST") && child.has_define("PATCHED"));
}

#[test]
fn long_parent_chains_and_deep_variants_resolve_while_cycles_stay_unresolved() {
    const CHAIN: usize = 1_000;
    let mut chain = serde_json::Map::new();
    chain.insert("m0".into(), json!({"+defines": ["ROOT"]}));
    for index in 1..CHAIN {
        chain.insert(format!("m{index}:m{}", index - 1), json!({}));
    }
    let (leaf, _) =
        definition(&[modern(Value::Object(chain))], &format!("m{}", CHAIN - 1)).unwrap();
    assert!(leaf.has_define("ROOT"));

    let cycle = modern(json!({"a:b": {}, "b:a": {}}));
    assert!(definition(&[cycle], "a").is_none());

    // Twelve nested variant levels, past any fixed nesting bound short of the JSON parser's.
    let mut fields = json!({"+defines": ["DEEPEST"]});
    for level in (0..12).rev() {
        fields = json!({"+defines": [format!("L{level}")], "variants": [{"inner": fields}]});
    }
    let (outer, table) = definition(&[modern(json!({ "outer": fields }))], "outer").unwrap();
    let mut current = &outer;
    while let Some(variant) = current.variants.first() {
        current = &table.definitions[variant.definition as usize];
    }
    assert!(current.has_define("DEEPEST") && current.has_define("L11"));
}

#[test]
fn materials_past_the_definition_bound_stay_unresolved_and_are_counted() {
    // Each material needs two entries: its variant and itself.
    let admitted = MAX_ENTITY_MATERIAL_DEFINITIONS / 2;
    let declarations = (0..=admitted)
        .map(|index| {
            (
                format!("m{index}"),
                json!({"+defines": [format!("D{index}")], "variants": [{"v": {"+defines": ["V"]}}]}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let mut states = MaterialStates::default();
    states.read(&modern(Value::Object(declarations)));
    let mut table = MaterialTable::default();
    for index in 0..admitted {
        let resolved = states.resolve(&format!("m{index}"), &mut table);
        assert!(resolved.is_some(), "{index}");
    }
    assert_eq!(table.unresolved(), 0);
    let past = states.resolve(&format!("m{admitted}"), &mut table);
    assert!(past.is_none());
    assert_eq!(table.unresolved(), 2, "the variant and its owner");
    assert_eq!(table.definitions.len(), MAX_ENTITY_MATERIAL_DEFINITIONS);
    // Earlier materials still resolve from the table.
    assert!(states.resolve("m0", &mut table).is_some());

    let declarations = (0..=assets::MAX_ENTITY_ASSET_SYMBOLS)
        .map(|index| (format!("n{index}"), json!({})))
        .collect::<serde_json::Map<_, _>>();
    let mut states = MaterialStates::default();
    states.read(&modern(Value::Object(declarations)));
    assert_eq!(states.unadmitted(), 1);
}

#[test]
fn every_name_of_a_long_chain_resolves_with_each_declaration_applied_once() {
    const CHAIN: usize = 4_000;
    let mut chain = serde_json::Map::new();
    chain.insert("m0".into(), json!({"+defines": ["ROOT"]}));
    for index in 1..CHAIN {
        chain.insert(format!("m{index}:m{}", index - 1), json!({}));
    }
    let mut states = MaterialStates::default();
    states.read(&modern(Value::Object(chain)));
    let mut table = MaterialTable::default();
    // Leaves first is the order that would re-walk the most without memoised links.
    for index in (0..CHAIN).rev() {
        assert!(states.resolve(&format!("m{index}"), &mut table).is_some());
    }
    assert_eq!(table.chains.applies, CHAIN);
}

#[test]
fn material_documents_past_the_parser_depth_limit_are_rejected() {
    // Resolution recurses once per variant nesting level; the JSON parser's recursion limit is
    // what bounds that depth, so it must stay enabled where material files are parsed.
    let nested = |levels: usize| {
        let mut fields = String::from("{}");
        for _ in 0..levels {
            fields = format!(r#"{{"variants": [{{"inner": {fields}}}]}}"#);
        }
        format!(r#"{{"materials": {{"version": "1.0.0", "deep": {fields}}}}}"#)
    };
    let path = std::path::Path::new("materials/deep.material");
    let shallow = pack_compile_input::json::parse_unique_json(path, nested(30).as_bytes())
        .expect("a deep but admitted document parses");
    let mut states = MaterialStates::default();
    states.read(&shallow);
    assert!(
        states
            .resolve("deep", &mut MaterialTable::default())
            .is_some()
    );
    assert!(
        pack_compile_input::json::parse_unique_json(path, nested(200).as_bytes()).is_err(),
        "a document deeper than the parser admits is an error, not a stack overflow"
    );
}

#[test]
fn a_long_chain_under_a_wide_variant_root_shares_one_variant_list() {
    const CHAIN: usize = 2_000;
    const VARIANTS: usize = 300;
    let mut chain = serde_json::Map::new();
    let variants = (0..VARIANTS)
        .map(|index| json!({ format!("v{index}"): {"+defines": [format!("V{index}")]} }))
        .collect::<Vec<_>>();
    chain.insert("m0".into(), json!({ "variants": variants }));
    for index in 1..CHAIN {
        chain.insert(format!("m{index}:m{}", index - 1), json!({}));
    }
    let mut states = MaterialStates::default();
    states.read(&modern(Value::Object(chain)));
    let mut table = MaterialTable::default();
    for index in (0..CHAIN).rev() {
        assert!(states.resolve(&format!("m{index}"), &mut table).is_some());
    }
    let links = table.chains.links.values().flatten().collect::<Vec<_>>();
    let resolutions = links
        .iter()
        .map(|resolved| Arc::as_ptr(resolved))
        .collect::<HashSet<_>>();
    let variant_lists = links
        .iter()
        .map(|resolved| Arc::as_ptr(&resolved.variants))
        .collect::<HashSet<_>>();
    assert!(
        resolutions.len() <= CHAIN,
        "{} resolutions",
        resolutions.len()
    );
    assert_eq!(
        variant_lists.len(),
        1,
        "every link inherits the root's list"
    );
    assert!(
        links
            .iter()
            .all(|resolved| resolved.variants.len() == VARIANTS)
    );
}

#[test]
fn dye_lookups_share_parent_chain_links_across_calls() {
    const CHAIN: usize = 1_000;
    let mut chain = serde_json::Map::new();
    chain.insert("m0:armor_leather".into(), json!({}));
    for index in 1..CHAIN {
        chain.insert(format!("m{index}:m{}", index - 1), json!({}));
    }
    let mut states = MaterialStates::default();
    states.read(&modern(Value::Object(chain)));
    let mut links = ChainMemo::default();
    for index in (0..CHAIN).rev() {
        assert_eq!(
            states.color_mask(&format!("m{index}"), &mut links),
            assets::EquipmentColorMask::Dye
        );
    }
    assert_eq!(links.applies, CHAIN);
}

/// Resolves every name in `names`, in order, from one modern file and returns the table.
fn resolve_all(file: Value, names: impl Iterator<Item = String>) -> MaterialTable {
    let mut states = MaterialStates::default();
    states.read(&modern(file));
    let mut table = MaterialTable::default();
    for name in names {
        assert!(states.resolve(&name, &mut table).is_some(), "{name}");
    }
    table
}

/// Distinct allocations of one shared field across every remembered link.
fn distinct<T: ?Sized>(
    table: &MaterialTable,
    field: impl Fn(&Resolved) -> Option<*const T>,
) -> usize {
    table
        .chains
        .links
        .values()
        .flatten()
        .filter_map(|resolved| field(resolved))
        .map(|pointer| pointer.cast::<()>())
        .collect::<HashSet<_>>()
        .len()
}

#[test]
fn a_long_chain_under_a_root_with_many_defines_or_vertex_fields_shares_them() {
    const CHAIN: usize = 2_000;
    let defines = (0..1_000)
        .map(|index| format!("{index:0>200}"))
        .collect::<Vec<_>>();
    let fields = (0..1_000)
        .map(|index| json!({"field": if index % 2 == 0 { "Position" } else { "UV0" }}))
        .collect::<Vec<_>>();
    let mut chain = serde_json::Map::new();
    chain.insert(
        "m0".into(),
        json!({"defines": defines, "vertexFields": fields}),
    );
    for index in 1..CHAIN {
        chain.insert(format!("m{index}:m{}", index - 1), json!({}));
    }
    let table = resolve_all(
        Value::Object(chain),
        (0..CHAIN).rev().map(|index| format!("m{index}")),
    );
    assert_eq!(table.chains.applies, CHAIN);
    assert_eq!(
        distinct(&table, |resolved| Some(Arc::as_ptr(&resolved.defines))),
        1
    );
    assert_eq!(
        distinct(&table, |resolved| resolved
            .vertex_fields
            .as_ref()
            .map(Arc::as_ptr)),
        1
    );
    // The interned carrier definitions still carry the complete lists.
    assert!(table.definitions.iter().all(|definition| {
        definition.defines.len() == 1_000
            && definition
                .vertex_fields
                .as_ref()
                .is_some_and(|fields| fields.len() == 1_000)
    }));
}

#[test]
fn two_long_chains_from_one_root_apply_each_declaration_once_and_share_its_lists() {
    const CHAIN: usize = 1_000;
    let defines = (0..500)
        .map(|index| format!("D{index}"))
        .collect::<Vec<_>>();
    let mut file = serde_json::Map::new();
    file.insert("root".into(), json!({ "defines": defines }));
    for branch in ["a", "b"] {
        file.insert(format!("{branch}1:root"), json!({}));
        for index in 2..=CHAIN {
            file.insert(format!("{branch}{index}:{branch}{}", index - 1), json!({}));
        }
    }
    let names = ["a", "b"].into_iter().flat_map(|branch| {
        (1..=CHAIN)
            .rev()
            .map(move |index| format!("{branch}{index}"))
    });
    let table = resolve_all(Value::Object(file), names);
    assert_eq!(table.chains.applies, 2 * CHAIN + 1);
    assert_eq!(
        distinct(&table, |resolved| Some(Arc::as_ptr(&resolved.defines))),
        1
    );
}

#[test]
fn actor_draws_use_a_materials_skinning_variant_and_otherwise_the_material() {
    let file = modern(json!({
        "owner": {"+defines": ["OWNER"], "variants": [{"skinning": {"+states": ["Blending"]}}]},
        "plain": {"+defines": ["PLAIN"]}
    }));
    let mut states = MaterialStates::default();
    states.read(&file);
    let mut table = MaterialTable::default();
    for (name, blends, define) in [("owner", true, "OWNER"), ("plain", false, "PLAIN")] {
        let (_, index) = states.resolve(name, &mut table).unwrap();
        let drawn = &table.definitions[index as usize];
        assert_eq!(drawn.has_state(State::Blending), blends, "{name}");
        assert!(drawn.has_define(define), "{name}");
    }
}

/// Resolves `name` from documents admitted by path, as compilation loads them.
fn listed_definition(documents: &[(&str, Value)], name: &str) -> Option<EntityMaterialDefinition> {
    let states =
        MaterialStates::from_documents(documents.iter().map(|(path, value)| (*path, value)));
    let mut table = MaterialTable::default();
    let (_, index) = states.resolve(name, &mut table)?;
    Some(table.definitions[index as usize].clone())
}

#[test]
fn list_defines_seed_every_root_of_the_listed_file() {
    let documents = [
        (
            list::ENTITY_MATERIAL_LIST_PATH,
            json!([{"path": "materials/listed.material", "tag": "ignored", "+defines": ["LISTED"]}]),
        ),
        (
            "materials/listed.material",
            modern(json!({
                "root": {"+defines": ["OWN"]},
                "child:root": {},
                "native:entity_alphatest": {},
                "replaced": {"defines": ["ONLY"]}
            })),
        ),
        ("materials/other.material", modern(json!({"other": {}}))),
    ];
    for name in ["root", "child", "native"] {
        let definition = listed_definition(&documents, name).unwrap();
        assert!(definition.has_define("LISTED"), "{name}");
    }
    let native = listed_definition(&documents, "native").unwrap();
    assert!(native.has_define("ALPHA_TEST"));
    let replaced = listed_definition(&documents, "replaced").unwrap();
    assert!(replaced.has_define("ONLY") && !replaced.has_define("LISTED"));
    let other = listed_definition(&documents, "other").unwrap();
    assert!(
        !other.has_define("LISTED"),
        "unlisted files have no list defines"
    );
}

#[test]
fn listed_files_load_before_unlisted_files() {
    let documents = [
        (
            list::ENTITY_MATERIAL_LIST_PATH,
            json!([{"path": "materials/z.material"}]),
        ),
        (
            "materials/a.material",
            modern(json!({"shared": {"+defines": ["UNLISTED"]}})),
        ),
        (
            "materials/z.material",
            modern(json!({"shared": {"+defines": ["LISTED"]}})),
        ),
    ];
    let shared = listed_definition(&documents, "shared").unwrap();
    assert!(shared.has_define("LISTED") && !shared.has_define("UNLISTED"));
}

#[test]
fn a_list_entry_whose_path_is_too_long_loads_nothing() {
    let path = format!(
        "materials/{}.material",
        "a".repeat(list::MAX_MATERIAL_LIST_PATH_BYTES)
    );
    let documents = [
        (
            list::ENTITY_MATERIAL_LIST_PATH,
            json!([{"path": path, "+defines": ["LISTED"]}]),
        ),
        (path.as_str(), modern(json!({"long": {}}))),
    ];
    let long = listed_definition(&documents, "long").unwrap();
    assert!(
        !long.has_define("LISTED"),
        "the file loads only as unlisted"
    );
}
