//! `cinnabar/material-bindings.json`: Cinnabar's data-only material extension.
//!
//! A material is a Bedrock material declaration (the same fields `.material` files accept, read
//! with version 1 rules) over an optional base material, plus named textures and typed
//! parameters. Bindings attach materials to entity bones or block material instances; the last
//! matching binding wins. Vanilla clients ignore the file. An invalid document is ignored whole,
//! leaving Bedrock materials unchanged.

use std::collections::BTreeMap;

use assets::{
    MAX_ENTITY_RENDER_PATTERN_BYTES,
    entity::material::{MAX_ENTITY_MATERIAL_DEFINITIONS, MAX_ENTITY_MATERIAL_NAME_BYTES},
};
use serde::Deserialize;
use serde_json::{Map, Value};

/// Where a resource pack declares the extension.
pub const MATERIAL_BINDINGS_PATH: &str = "cinnabar/material-bindings.json";

/// The schema version this client reads.
const FORMAT_VERSION: &str = "1.0.0";

/// A validated extension document.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Extension {
    pub(crate) materials: BTreeMap<Box<str>, Material>,
    pub(crate) entity_bindings: Vec<EntityBinding>,
    pub(crate) block_bindings: Vec<BlockBinding>,
}

/// One extension material.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Material {
    /// Bedrock material it derives from; `None` starts from the constructor defaults.
    pub(crate) base: Option<Box<str>>,
    /// Bedrock declaration fields, applied with version 1 rules.
    pub(crate) fields: Map<String, Value>,
    /// Texture names to pack-relative resource paths, without extension.
    pub(crate) textures: BTreeMap<Box<str>, Box<str>>,
    pub(crate) parameters: BTreeMap<Box<str>, Parameter>,
}

/// A typed value a host program may read.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Parameter {
    Scalar(f32),
    /// Two to four components.
    Vector(Vec<f32>),
    /// Linear RGBA.
    Color([f32; 4]),
    Boolean(bool),
}

/// Attaches a material to bones of an entity or attachable.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EntityBinding {
    pub(crate) entity: Box<str>,
    /// Render-controller name pattern; `None` matches every controller.
    pub(crate) render_controller: Option<Box<str>>,
    /// Lowercase bone pattern; `None` matches every bone.
    pub(crate) bone: Option<Box<str>>,
    pub(crate) material: Box<str>,
}

/// A block binding with its material resolved, ready for the block renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedBlockBinding {
    /// Exact block identifier.
    pub block: Box<str>,
    /// State values the block must have; other states are unconstrained.
    pub states: Map<String, Value>,
    /// Material-instance name pattern; `None` matches every instance.
    pub material_instance: Option<Box<str>>,
    /// The resolved Bedrock material; [`EntityMaterialDefinition::render_state`] lowers it.
    ///
    /// [`EntityMaterialDefinition::render_state`]: assets::entity::material::EntityMaterialDefinition::render_state
    pub definition: assets::entity::material::EntityMaterialDefinition,
    pub shading: assets::EntityRenderMaterialState,
    /// The material's glow and emissive masks as terrain texture keys, and their strength.
    pub masks: BlockMaskPaths,
}

/// A block material's mask textures, named as `terrain_texture.json` keys as block material
/// instances name their textures.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockMaskPaths {
    pub glow: Option<Box<str>>,
    pub emissive: Option<Box<str>>,
    pub strength: f32,
}

/// Resolves a pack's block bindings against its material documents, keyed by pack path and
/// loaded as the entity material list orders them.
pub fn block_bindings(
    material_documents: &[(String, Value)],
    document: &Value,
) -> Vec<ResolvedBlockBinding> {
    super::MaterialStates::block_bindings(material_documents, document)
}

/// Attaches a material to material instances of blocks in matching states.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BlockBinding {
    pub(crate) block: Box<str>,
    pub(crate) states: Map<String, Value>,
    pub(crate) material_instance: Option<Box<str>>,
    pub(crate) material: Box<str>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    format_version: String,
    #[serde(default)]
    materials: BTreeMap<String, MaterialDocument>,
    #[serde(default)]
    bindings: Vec<BindingDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterialDocument {
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    material: Map<String, Value>,
    #[serde(default)]
    textures: BTreeMap<String, String>,
    #[serde(default)]
    parameters: BTreeMap<String, Parameter>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingDocument {
    material: String,
    entity: Option<String>,
    render_controller: Option<String>,
    bone: Option<String>,
    block: Option<String>,
    states: Option<Map<String, Value>>,
    material_instance: Option<String>,
}

/// Checks a document, or says why the client ignores it.
pub fn validate(value: &Value) -> Result<(), Box<str>> {
    parse(value).map(|_| ())
}

/// Validates a document, or says why it is ignored.
pub(crate) fn parse(value: &Value) -> Result<Extension, Box<str>> {
    let document = Document::deserialize(value)
        .map_err(|error| format!("schema: {error}").into_boxed_str())?;
    if document.format_version != FORMAT_VERSION {
        return Err(format!("unsupported format_version {}", document.format_version).into());
    }
    if document.materials.len() > MAX_ENTITY_MATERIAL_DEFINITIONS {
        return Err("too many materials".into());
    }
    let mut extension = Extension::default();
    for (name, material) in document.materials {
        if !is_name(&name) || material.base.as_deref().is_some_and(|base| !is_name(base)) {
            return Err(format!("invalid material name {name}").into());
        }
        if !super::lists_are_well_formed(&material.material) {
            return Err(format!("{name}: malformed state or define list").into());
        }
        if let Some((texture, _)) = material
            .textures
            .iter()
            .find(|(texture, path)| texture.is_empty() || !is_resource_path(path))
        {
            return Err(format!("{name}: invalid texture {texture}").into());
        }
        if let Some((parameter, _)) = material
            .parameters
            .iter()
            .find(|(parameter, value)| parameter.is_empty() || !is_finite(value))
        {
            return Err(format!("{name}: invalid parameter {parameter}").into());
        }
        extension.materials.insert(
            name.into(),
            Material {
                base: material.base.map(Into::into),
                fields: material.material,
                textures: material
                    .textures
                    .into_iter()
                    .map(|(name, path)| (name.into(), path.into()))
                    .collect(),
                parameters: material
                    .parameters
                    .into_iter()
                    .map(|(name, value)| (name.into(), value))
                    .collect(),
            },
        );
    }
    for binding in document.bindings {
        if !extension.materials.contains_key(binding.material.as_str()) {
            return Err(format!("binding names unknown material {}", binding.material).into());
        }
        let patterns = [
            &binding.render_controller,
            &binding.bone,
            &binding.material_instance,
        ];
        if patterns
            .into_iter()
            .flatten()
            .any(|pattern| pattern.is_empty() || pattern.len() > MAX_ENTITY_RENDER_PATTERN_BYTES)
        {
            return Err("binding pattern is empty or too long".into());
        }
        match (binding.entity, binding.block) {
            (Some(entity), None)
                if binding.states.is_none() && binding.material_instance.is_none() =>
            {
                extension.entity_bindings.push(EntityBinding {
                    entity: entity.into(),
                    render_controller: binding.render_controller.map(Into::into),
                    bone: binding.bone.map(|bone| bone.to_ascii_lowercase().into()),
                    material: binding.material.into(),
                });
            }
            (None, Some(block))
                if binding.render_controller.is_none() && binding.bone.is_none() =>
            {
                let states = binding.states.unwrap_or_default();
                if states.values().any(|value| {
                    !matches!(value, Value::Bool(_) | Value::Number(_) | Value::String(_))
                }) {
                    return Err(format!("{block}: block state values must be scalars").into());
                }
                extension.block_bindings.push(BlockBinding {
                    block: block.into(),
                    states,
                    material_instance: binding.material_instance.map(Into::into),
                    material: binding.material.into(),
                });
            }
            _ => return Err("a binding targets exactly one entity or one block".into()),
        }
    }
    Ok(extension)
}

/// Texture names whose masks Cinnabar draws, and the parameter that scales them.
const GLOW_MASK: &str = "glow_mask";
const EMISSIVE_MASK: &str = "emissive_mask";
const STRENGTH: &str = "strength";

impl Material {
    /// The mask textures a block draw samples, as terrain texture keys.
    pub(crate) fn mask_paths(&self) -> BlockMaskPaths {
        BlockMaskPaths {
            glow: self.textures.get(GLOW_MASK).cloned(),
            emissive: self.textures.get(EMISSIVE_MASK).cloned(),
            strength: match self.parameters.get(STRENGTH) {
                Some(Parameter::Scalar(strength)) => *strength,
                _ => 1.0,
            },
        }
    }

    /// The masks this material draws, with `source` mapping a resource path to its entity
    /// source; `None` without any mask. A mask whose texture is not loaded is omitted.
    pub(crate) fn masks(
        &self,
        source: impl Fn(&str) -> Option<u32>,
    ) -> Option<assets::EntityMaterialMasks> {
        let texture = |name| self.textures.get(name).and_then(|path| source(path));
        let glow = texture(GLOW_MASK);
        let emissive = texture(EMISSIVE_MASK);
        (glow.is_some() || emissive.is_some()).then(|| assets::EntityMaterialMasks {
            glow,
            emissive,
            strength: match self.parameters.get(STRENGTH) {
                Some(Parameter::Scalar(strength)) => *strength,
                _ => 1.0,
            },
        })
    }
}

impl Extension {
    /// Bindings for `entity`'s `controller`, in document order, as (bone pattern, material).
    pub(crate) fn entity_rules<'a>(
        &'a self,
        entity: &'a str,
        controller: &'a str,
    ) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
        self.entity_bindings
            .iter()
            .filter(move |binding| {
                binding.entity.as_ref() == entity
                    && binding.render_controller.as_deref().is_none_or(|pattern| {
                        assets::entity_render_pattern_matches(pattern, controller)
                    })
            })
            .map(|binding| {
                (
                    binding.bone.as_deref().unwrap_or("*"),
                    binding.material.as_ref(),
                )
            })
    }
}

fn is_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_ENTITY_MATERIAL_NAME_BYTES
}

/// A pack-relative resource path that cannot leave the pack.
fn is_resource_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn is_finite(value: &Parameter) -> bool {
    match value {
        Parameter::Scalar(value) => value.is_finite(),
        Parameter::Vector(values) => {
            (2..=4).contains(&values.len()) && values.iter().all(|value| value.is_finite())
        }
        Parameter::Color(values) => values.iter().all(|value| value.is_finite()),
        Parameter::Boolean(_) => true,
    }
}

#[cfg(test)]
#[path = "extension_schema_tests.rs"]
mod schema_tests;

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn document(material: Value, binding: Value) -> Value {
        json!({
            "format_version": "1.0.0",
            "materials": {"example:glow": material},
            "bindings": [binding]
        })
    }

    #[test]
    fn valid_documents_keep_materials_resources_and_bindings_in_order() {
        let value = json!({
            "format_version": "1.0.0",
            "materials": {"example:glow": {
                "base": "entity_alphatest",
                "material": {"+states": ["Blending"]},
                "textures": {"glow_mask": "textures/example/glow"},
                "parameters": {"strength": {"scalar": 2.0}, "lit": {"boolean": true},
                               "scroll": {"vector": [0.5, 0.0]}}
            }},
            "bindings": [
                {"entity": "example:moth", "material": "example:glow"},
                {"entity": "example:moth", "bone": "Wing*", "material": "example:glow"},
                {"block": "example:lamp", "states": {"example:lit": true},
                 "material_instance": "*", "material": "example:glow"}
            ]
        });
        let extension = parse(&value).unwrap();
        let material = &extension.materials["example:glow"];
        assert_eq!(material.base.as_deref(), Some("entity_alphatest"));
        assert_eq!(material.parameters["strength"], Parameter::Scalar(2.0));
        assert_eq!(
            extension
                .entity_rules("example:moth", "controller.render.moth")
                .collect::<Vec<_>>(),
            [("*", "example:glow"), ("wing*", "example:glow")]
        );
        assert_eq!(extension.block_bindings.len(), 1);
    }

    #[test]
    fn malformed_documents_are_rejected_whole() {
        let entity = json!({"entity": "example:moth", "material": "example:glow"});
        for value in [
            json!({"format_version": "1.1.0"}),
            json!({"format_version": "1.0.0", "extra": 1}),
            document(json!({"program": "emissive"}), entity.clone()),
            document(json!({"material": {"states": "Blending"}}), entity.clone()),
            document(json!({"textures": {"glow": "../outside"}}), entity.clone()),
            document(json!({"textures": {"glow": "/absolute"}}), entity.clone()),
            document(
                json!({"parameters": {"scroll": {"vector": [1.0]}}}),
                entity.clone(),
            ),
            document(
                json!({}),
                json!({"entity": "example:moth", "material": "example:other"}),
            ),
            document(json!({}), json!({"material": "example:glow"})),
            document(
                json!({}),
                json!({"entity": "example:moth", "block": "example:lamp", "material": "example:glow"}),
            ),
            document(
                json!({}),
                json!({"entity": "example:moth", "material_instance": "*", "material": "example:glow"}),
            ),
            document(
                json!({}),
                json!({"block": "example:lamp", "states": {"facing": [1]}, "material": "example:glow"}),
            ),
            document(
                json!({}),
                json!({"entity": "example:moth", "bone": "", "material": "example:glow"}),
            ),
        ] {
            assert!(validate(&value).is_err(), "{value}");
        }
    }
}
