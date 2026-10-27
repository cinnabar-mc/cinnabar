//! Keeps the published JSON Schema for `cinnabar/material-bindings.json` in step with the
//! parser: the field and variant names come from the parser's own serde definitions, and the
//! same documents are checked against both.

use std::path::Path;

use serde::de::{self, Deserializer, Visitor};
use serde_json::{Value, json};

use super::*;

/// The schema checked in beside the extension's reference documentation.
fn schema() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/reference/cinnabar-material-bindings-1.0.0.schema.json");
    serde_json::from_slice(&std::fs::read(&path).expect("the schema is checked in"))
        .expect("the schema is JSON")
}

/// Records the field or variant names a serde-derived type asks a deserializer for.
struct Names(std::cell::RefCell<Vec<&'static str>>);

#[derive(Debug)]
struct Recorded;

impl std::fmt::Display for Recorded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("recorded")
    }
}

impl std::error::Error for Recorded {}

impl de::Error for Recorded {
    fn custom<T: std::fmt::Display>(_: T) -> Self {
        Self
    }
}

impl<'de> Deserializer<'de> for &Names {
    type Error = Recorded;

    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Recorded> {
        Err(Recorded)
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        fields: &'static [&'static str],
        _: V,
    ) -> Result<V::Value, Recorded> {
        self.0.borrow_mut().extend_from_slice(fields);
        Err(Recorded)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        variants: &'static [&'static str],
        _: V,
    ) -> Result<V::Value, Recorded> {
        self.0.borrow_mut().extend_from_slice(variants);
        Err(Recorded)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
        option unit unit_struct newtype_struct seq tuple tuple_struct map identifier
        ignored_any
    }
}

/// The names `T`'s derived `Deserialize` reads, sorted.
fn names<T: for<'de> Deserialize<'de>>() -> Vec<&'static str> {
    let names = Names(Default::default());
    let _ = T::deserialize(&names);
    let mut names = names.0.into_inner();
    names.sort_unstable();
    names
}

/// The sorted property names of the schema object at `pointer`.
fn properties(schema: &Value, pointer: &str) -> Vec<String> {
    let mut names = schema
        .pointer(pointer)
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("schema has {pointer}"))
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
}

#[test]
fn schema_fields_variants_and_limits_are_the_parsers() {
    let schema = schema();
    assert_eq!(names::<Document>(), properties(&schema, "/properties"));
    assert_eq!(
        names::<MaterialDocument>(),
        properties(&schema, "/$defs/material/properties")
    );
    for binding in ["entity_binding", "block_binding"] {
        assert_eq!(
            names::<BindingDocument>(),
            properties(&schema, &format!("/$defs/{binding}/properties")),
            "{binding}"
        );
    }
    let mut variants = schema["$defs"]["parameter"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|variant| variant["required"].as_array().unwrap().clone())
        .map(|name| name.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    variants.sort_unstable();
    assert_eq!(names::<Parameter>(), variants);
    assert_eq!(
        schema["properties"]["format_version"]["const"],
        FORMAT_VERSION
    );
    assert_eq!(
        schema["properties"]["materials"]["maxProperties"],
        MAX_ENTITY_MATERIAL_DEFINITIONS
    );
    for definition in ["name", "optional_name"] {
        assert_eq!(
            schema["$defs"][definition]["maxLength"],
            MAX_ENTITY_MATERIAL_NAME_BYTES
        );
    }
    assert_eq!(
        schema["$defs"]["optional_pattern"]["maxLength"],
        MAX_ENTITY_RENDER_PATTERN_BYTES
    );
    assert_eq!(
        schema["$defs"]["block_binding"]["properties"]["condition"]["maxLength"],
        crate::entity::molang::MAX_BLOCK_MOLANG_BYTES
    );
    let mut lists = properties(&schema, "/$defs/declaration/properties");
    lists.sort_unstable();
    let mut checked = super::super::fields::LIST_KEYS.to_vec();
    checked.sort_unstable();
    assert_eq!(lists, checked);
}

/// Whether `value` satisfies `schema`, for the keywords the published schema uses. An unknown
/// keyword fails the test rather than being ignored.
fn valid(root: &Value, schema: &Value, value: &Value) -> bool {
    let Some(schema) = schema.as_object() else {
        return schema.as_bool().unwrap_or(false);
    };
    let length = |value: &Value| value.as_str().map(|text| text.chars().count());
    let number = |key: &str| schema.get(key).and_then(Value::as_u64).map(|n| n as usize);
    schema
        .iter()
        .all(|(keyword, argument)| match keyword.as_str() {
            "$schema" | "$id" | "$defs" | "title" | "description" => true,
            "$ref" => {
                let pointer = argument.as_str().unwrap().trim_start_matches('#');
                valid(root, root.pointer(pointer).expect("local reference"), value)
            }
            "type" => {
                let accepts = |kind: &Value| match kind.as_str().unwrap() {
                    "object" => value.is_object(),
                    "array" => value.is_array(),
                    "string" => value.is_string(),
                    "number" => value.is_number(),
                    "boolean" => value.is_boolean(),
                    "null" => value.is_null(),
                    other => panic!("type {other}"),
                };
                match argument {
                    Value::Array(kinds) => kinds.iter().any(accepts),
                    kind => accepts(kind),
                }
            }
            "const" => value == argument,
            "required" => argument
                .as_array()
                .unwrap()
                .iter()
                .all(|key| value.get(key.as_str().unwrap()).is_some() || !value.is_object()),
            "properties" => value.as_object().is_none_or(|object| {
                argument.as_object().unwrap().iter().all(|(key, schema)| {
                    object
                        .get(key)
                        .is_none_or(|member| valid(root, schema, member))
                })
            }),
            "additionalProperties" => value.as_object().is_none_or(|object| {
                let declared = schema.get("properties").and_then(Value::as_object);
                object
                    .iter()
                    .filter(|(key, _)| declared.is_none_or(|declared| !declared.contains_key(*key)))
                    .all(|(_, member)| valid(root, argument, member))
            }),
            "propertyNames" => value.as_object().is_none_or(|object| {
                object
                    .keys()
                    .all(|key| valid(root, argument, &Value::String(key.clone())))
            }),
            "maxProperties" => value
                .as_object()
                .is_none_or(|object| object.len() <= number(keyword).unwrap()),
            "items" => value
                .as_array()
                .is_none_or(|items| items.iter().all(|item| valid(root, argument, item))),
            "minItems" => value
                .as_array()
                .is_none_or(|items| items.len() >= number(keyword).unwrap()),
            "maxItems" => value
                .as_array()
                .is_none_or(|items| items.len() <= number(keyword).unwrap()),
            "minLength" => length(value).is_none_or(|length| length >= number(keyword).unwrap()),
            "maxLength" => length(value).is_none_or(|length| length <= number(keyword).unwrap()),
            "pattern" => value.as_str().is_none_or(|text| {
                regex::Regex::new(argument.as_str().unwrap())
                    .unwrap()
                    .is_match(text)
            }),
            "not" => !valid(root, argument, value),
            "oneOf" => {
                argument
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|schema| valid(root, schema, value))
                    .count()
                    == 1
            }
            other => panic!("the schema uses unchecked keyword {other}"),
        })
}

/// Documents both the parser and the schema accept.
fn accepted() -> Vec<Value> {
    vec![
        json!({"format_version": "1.0.0"}),
        json!({
            "format_version": "1.0.0",
            "materials": {"example:glow": {
                "base": "entity_alphatest",
                "material": {"+states": ["Blending"], "+defines": ["USE_EMISSIVE"], "blendSrc": "One"},
                "textures": {"glow_mask": "textures/example/glow"},
                "parameters": {"strength": {"scalar": 2.0}, "lit": {"boolean": true},
                               "scroll": {"vector": [0.5, 0.0]}, "tint": {"color": [1.0, 0.5, 0.0, 1.0]}}
            }, "example:plain": {"base": null, "material": {"states": null}}},
            "bindings": [
                {"entity": "example:moth", "material": "example:glow"},
                {"entity": "example:moth", "render_controller": "controller.*", "bone": "wing*",
                 "block": null, "material": "example:glow"},
                {"block": "example:lamp", "states": {"example:lit": true, "level": 3, "face": "up"},
                 "material_instance": "*", "material": "example:plain"},
                {"block": "example:lamp", "states": null, "material": "example:glow"},
                {"block": "example:lamp", "condition": "q.block_state('example:level') > 2",
                 "material": "example:glow"}
            ]
        }),
    ]
}

/// Documents both reject.
fn rejected() -> Vec<Value> {
    let with = |material: Value, binding: Value| {
        json!({"format_version": "1.0.0", "materials": {"example:glow": material},
               "bindings": [binding]})
    };
    let entity = json!({"entity": "example:moth", "material": "example:glow"});
    vec![
        json!({}),
        json!({"format_version": "1.1.0"}),
        json!({"format_version": "1.0.0", "extra": 1}),
        with(json!({"program": "emissive"}), entity.clone()),
        with(json!({"material": {"states": "Blending"}}), entity.clone()),
        with(json!({"material": {"-defines": [1]}}), entity.clone()),
        with(json!({"textures": {"glow": "../outside"}}), entity.clone()),
        with(json!({"textures": {"glow": "a/./b"}}), entity.clone()),
        with(json!({"textures": {"glow": "/absolute"}}), entity.clone()),
        with(json!({"textures": {"glow": "a\\b"}}), entity.clone()),
        with(json!({"textures": {"": "a"}}), entity.clone()),
        with(
            json!({"parameters": {"scroll": {"vector": [1.0]}}}),
            entity.clone(),
        ),
        with(
            json!({"parameters": {"tint": {"color": [1.0, 1.0, 1.0]}}}),
            entity.clone(),
        ),
        with(
            json!({"parameters": {"two": {"scalar": 1.0, "boolean": true}}}),
            entity.clone(),
        ),
        with(json!({"base": ""}), entity.clone()),
        with(json!({}), json!({"material": "example:glow"})),
        with(
            json!({}),
            json!({"entity": "example:moth", "block": "example:lamp", "material": "example:glow"}),
        ),
        with(
            json!({}),
            json!({"entity": "example:moth", "material_instance": "*", "material": "example:glow"}),
        ),
        with(
            json!({}),
            json!({"block": "example:lamp", "bone": "x", "material": "example:glow"}),
        ),
        with(
            json!({}),
            json!({"block": "example:lamp", "states": {"facing": [1]}, "material": "example:glow"}),
        ),
        with(
            json!({}),
            json!({"entity": "example:moth", "bone": "", "material": "example:glow"}),
        ),
        with(
            json!({}),
            json!({"entity": "example:moth", "condition": "1.0", "material": "example:glow"}),
        ),
        with(
            json!({}),
            json!({"entity": "example:moth", "bone": "b".repeat(65), "material": "example:glow"}),
        ),
    ]
}

#[test]
fn the_schema_and_the_parser_accept_the_same_documents() {
    let schema = schema();
    for document in accepted() {
        assert!(validate(&document).is_ok(), "parser rejects {document}");
        assert!(
            valid(&schema, &schema, &document),
            "schema rejects {document}"
        );
    }
    for document in rejected() {
        assert!(validate(&document).is_err(), "parser accepts {document}");
        assert!(
            !valid(&schema, &schema, &document),
            "schema accepts {document}"
        );
    }
    // The checks the schema documents but cannot express.
    let unknown = json!({"format_version": "1.0.0",
        "bindings": [{"entity": "example:moth", "material": "example:missing"}]});
    assert!(validate(&unknown).is_err() && valid(&schema, &schema, &unknown));
    let unparsable = json!({"format_version": "1.0.0", "materials": {"example:glow": {}},
        "bindings": [{"block": "example:lamp", "condition": "(((", "material": "example:glow"}]});
    assert!(validate(&unparsable).is_err() && valid(&schema, &schema, &unparsable));
}

#[test]
fn the_documented_sample_pack_loads_through_the_parser_and_the_schema() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/reference/cinnabar-material-sample");
    let read = |path: &str| -> Value {
        serde_json::from_slice(&std::fs::read(root.join(path)).expect("sample file")).unwrap()
    };
    let document = read(MATERIAL_BINDINGS_PATH);
    let schema = schema();
    assert!(valid(&schema, &schema, &document));
    let extension = parse(&document).expect("the sample is a valid document");
    assert_eq!(
        extension
            .entity_rules("minecraft:pig", "controller.render.pig")
            .collect::<Vec<_>>(),
        [("head", "sample:glow")]
    );
    let blocks = block_bindings(&[], &document);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].block.as_ref(), "sample:lamp");
    assert_eq!(blocks[0].states["sample:lit"], json!(true));
    let glow = blocks[0]
        .masks
        .glow
        .as_deref()
        .expect("the lamp draws a glow mask");
    let terrain = read("textures/terrain_texture.json");
    assert!(
        terrain["texture_data"].get(glow).is_some(),
        "the glow mask names a terrain texture key the sample declares"
    );
}
