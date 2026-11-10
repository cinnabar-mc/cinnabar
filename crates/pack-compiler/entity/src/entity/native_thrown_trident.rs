//! Thrown tridents, which vanilla draws with a native renderer rather than a render controller.
//! The generated controller only selects the entity's own model and texture; client
//! presentation places the model along the actor's flight rotation as the native renderer does.
use super::painting::{encode_document, replace_with_documents, unused_source_path};
use super::*;
use assets::facts::{entity::NativeEntityClass, pack::ALPHA_TEST_MATERIAL};
use serde_json::json;

const CONTROLLER: &str = "controller.render.cinnabar.thrown_trident";

/// Gives a client thrown trident that names no render controller one that draws its default
/// geometry with its default texture. Authored controllers are kept as they are.
pub(super) fn complete_bindings(
    root: &Path,
    sources: &mut Vec<EntityAssetSource>,
    payloads: &mut SourcePayloads,
    symbols: &mut BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    geometries: &mut BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
    remaining_budget: Option<(usize, usize)>,
) -> Result<usize, AssetError> {
    let Some(identifier) = NativeEntityClass::ThrownTrident.identifier() else {
        return Ok(0);
    };
    let mut authored = symbols
        .keys()
        .filter(|(kind, name, _)| *kind == EntityAssetKind::Entity && &**name == identifier);
    let (Some(key), None) = (authored.next(), authored.next()) else {
        return Ok(0);
    };
    let key = key.clone();
    let Some(bytes) = payloads.get(&key.2) else {
        return Ok(0);
    };
    let mut value = parse_unique_json(Path::new(key.2.as_ref()), bytes)?;
    let Some(description) = value
        .get_mut("minecraft:client_entity")
        .and_then(|entity| entity.get_mut("description"))
        .and_then(Value::as_object_mut)
    else {
        return Ok(0);
    };
    let named = |field: &str| {
        description
            .get(field)
            .and_then(|names| names.get("default"))
            .is_some_and(Value::is_string)
    };
    if description.contains_key("render_controllers") || !named("geometry") || !named("textures") {
        return Ok(0);
    }
    description
        .entry("materials")
        .or_insert_with(|| json!({ "default": ALPHA_TEST_MATERIAL }));
    description.insert("render_controllers".into(), json!([CONTROLLER]));
    let documents = vec![
        encode_document(
            unused_source_path("entity/_cinnabar/thrown_trident.json".into(), payloads),
            value,
        )?,
        encode_document(
            unused_source_path(
                "render_controllers/_cinnabar/thrown_trident.json".into(),
                payloads,
            ),
            json!({ "format_version": "1.8.0", "render_controllers": { CONTROLLER: {
                "geometry": "Geometry.default",
                "materials": [{ "*": "Material.default" }],
                "textures": ["Texture.default"],
            }}}),
        )?,
    ];
    replace_with_documents(
        root,
        sources,
        payloads,
        symbols,
        geometries,
        remaining_budget,
        &[key],
        documents,
    )
}

#[cfg(test)]
mod tests;
