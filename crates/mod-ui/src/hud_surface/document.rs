//! Splits a HUD surface's root into independently cached top-level controls and records which
//! bindings and collections each one reads.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use json_ui::{Catalog, Context};
use serde_json::{Map, Value};
use ui::mod_panel::{Surface, definitions::Definitions, projected_visible_name};

/// Local name prefix of the generated per-control part definitions.
const PART_PREFIX: &str = "cinnabar_hud_part_";

/// One independently laid-out and cached piece of a HUD surface.
#[derive(Clone, Debug)]
pub(crate) struct PartSpec {
    /// The screen reference the part renders.
    pub reference: String,
    /// Global bindings the part reads, including host-published ones.
    pub bindings: BTreeSet<String>,
    /// Collections whose grids the part contains.
    pub collections: BTreeSet<String>,
    /// A projected binding that only moves the part's top-level control.
    pub floating: Option<Floating>,
    /// The root child the part draws; none for a root kept whole or an embedded control.
    pub child: Option<usize>,
    /// The child's literal draw layer, which orders the parts.
    pub layer: i64,
}

/// A projected `#offset` binding and its optional `#visible` companion, read only by a part's
/// top-level control. The part is laid out once and its cached artwork moves instead.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Floating {
    pub offset: String,
    pub visible: Option<String>,
}

/// A HUD surface's catalog and its parts in paint order.
pub(super) struct Document {
    pub catalog: Arc<Catalog>,
    pub parts: Vec<PartSpec>,
}

impl Document {
    /// Validates `surface` as a HUD surface and splits its root into parts. A root whose
    /// children's layout depends on each other, or that draws itself, stays one part.
    pub fn build(surface: &Surface) -> Result<Self, String> {
        let mut document: Value =
            serde_json::from_str(&surface.validated_document(ui::mod_panel::SurfaceRole::Hud)?)
                .map_err(|error| error.to_string())?;
        let (namespace, name) = surface
            .screen
            .split_once('.')
            .ok_or("surface screen must be qualified")?;
        let root = document[name].clone();
        let children = root
            .get("controls")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut parts = Vec::new();
        if splittable(&root, &children) {
            let mut ordered: Vec<(i64, usize, Value)> = children
                .into_iter()
                .enumerate()
                .map(|(index, child)| (layer(&child), index, child))
                .collect();
            ordered.sort_by_key(|(layer, index, _)| (*layer, *index));
            let object = document
                .as_object_mut()
                .expect("resolved documents are objects");
            for (layer, index, child) in ordered {
                let local = format!("{PART_PREFIX}{index}");
                if object.contains_key(&local) {
                    return Err("surface document reserves HUD part names".into());
                }
                let mut part = root.clone();
                part["controls"] = Value::Array(vec![child.clone()]);
                let floating = child
                    .as_object()
                    .and_then(|named| named.values().next())
                    .and_then(|body| floating(surface, &part, body));
                parts.push(PartSpec {
                    reference: format!("{namespace}.{local}"),
                    bindings: BTreeSet::new(),
                    collections: BTreeSet::new(),
                    floating,
                    child: Some(index),
                    layer,
                });
                object.insert(local, part);
            }
        } else {
            parts.push(PartSpec {
                reference: surface.screen.clone(),
                bindings: BTreeSet::new(),
                collections: BTreeSet::new(),
                floating: None,
                child: None,
                layer: 0,
            });
        }
        for part in &mut parts {
            let local = part.reference.split_once('.').expect("qualified").1;
            reads(
                &document[local],
                &document,
                &mut part.bindings,
                &mut part.collections,
            );
            if let Some(floating) = &part.floating {
                part.bindings.insert(floating.offset.clone());
                part.bindings.extend(floating.visible.clone());
            }
        }
        let catalog = catalog(&document.to_string())?;
        for part in &parts {
            let resolution = json_ui::resolve(&catalog, &part.reference, &Context::default());
            if resolution.control.is_none() || !resolution.diagnostics.is_empty() {
                return Err(format!(
                    "HUD surface part cannot be resolved: {}",
                    resolution.diagnostics.join("; ")
                ));
            }
        }
        Ok(Self {
            catalog: Arc::new(catalog),
            parts,
        })
    }

    /// Builds the part that embeds the local control `control` of `surface`'s document. With
    /// the document's `definitions`, it reads only those the control's resolution reads.
    pub fn embedded(
        surface: &Surface,
        definitions: Option<&Definitions>,
        control: &str,
    ) -> Result<(Arc<Catalog>, PartSpec), String> {
        let (embedded, document) = match definitions {
            Some(definitions) => Surface {
                screen: surface.screen.clone(),
                document: definitions.document(&definitions.reads(control), None),
                bindings: Default::default(),
                transitions: Default::default(),
                collections: surface.collections.clone(),
            }
            .embedded_document(control)?,
            None => surface.embedded_document(control)?,
        };
        let document: Value = serde_json::from_str(&document).map_err(|error| error.to_string())?;
        let mut part = PartSpec {
            reference: embedded.screen,
            bindings: BTreeSet::new(),
            collections: BTreeSet::new(),
            floating: None,
            child: None,
            layer: 0,
        };
        reads(
            &document[control],
            &document,
            &mut part.bindings,
            &mut part.collections,
        );
        Ok((Arc::new(catalog(&document.to_string())?), part))
    }

    /// Builds the part for root child `index` of a split root, as [`Self::build`] would, by
    /// resolving only the root's own properties, that child and the definitions it reads.
    /// Errs for a root with a base or a child that must not be split from its siblings.
    pub fn child(
        surface: &Surface,
        definitions: &Definitions,
        index: usize,
    ) -> Result<(Arc<Catalog>, PartSpec), String> {
        let (namespace, name) = surface
            .screen
            .split_once('.')
            .ok_or("surface screen must be qualified")?;
        let root = definitions
            .definitions
            .get(name)
            .filter(|root| root.base.is_none())
            .ok_or("HUD surface root cannot be resolved by child")?;
        let entry = root
            .body
            .get("controls")
            .and_then(Value::as_array)
            .and_then(|children| children.get(index))
            .ok_or("HUD surface root child is missing")?;
        let local = format!("{PART_PREFIX}{index}");
        if definitions.definitions.contains_key(&local) {
            return Err("surface document reserves HUD part names".into());
        }
        let mut body = root.body.clone();
        body["controls"] = Value::Array(vec![entry.clone()]);
        let mut read = BTreeSet::new();
        for reference in definitions.references(&body) {
            read.extend(definitions.reads(&reference));
        }
        let partial = Surface {
            screen: surface.screen.clone(),
            document: definitions.document(&read, Some((&local, &body))),
            bindings: Default::default(),
            transitions: Default::default(),
            collections: surface.collections.clone(),
        };
        let (_, document) = partial.embedded_document(&local)?;
        let document: Value = serde_json::from_str(&document).map_err(|error| error.to_string())?;
        let part = &document[&local];
        let child = part
            .get("controls")
            .and_then(Value::as_array)
            .and_then(|children| children.first())
            .ok_or("HUD surface root child cannot be resolved")?;
        let child_body = child
            .as_object()
            .and_then(|named| named.values().next())
            .ok_or("HUD surface root child cannot be resolved")?;
        if measured_from(child_body.get("size"), &["%sm"]) {
            return Err("HUD surface root child is measured from its siblings".into());
        }
        if !flat_descendant_layers(child_body) {
            return Err("HUD surface descendant layers must paint with their root".into());
        }
        let mut spec = PartSpec {
            reference: format!("{namespace}.{local}"),
            bindings: BTreeSet::new(),
            collections: BTreeSet::new(),
            floating: floating(surface, part, child_body),
            child: Some(index),
            layer: layer(child),
        };
        reads(part, &document, &mut spec.bindings, &mut spec.collections);
        if let Some(floating) = &spec.floating {
            spec.bindings.insert(floating.offset.clone());
            spec.bindings.extend(floating.visible.clone());
        }
        Ok((Arc::new(catalog(&document.to_string())?), spec))
    }
}

/// Loads a canonical static document into a private catalog.
fn catalog(document: &str) -> Result<Catalog, String> {
    let definitions = br#"{"ui_defs":["ui/extension_surface.json"]}"#;
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", &b"{}"[..]),
        ("ui/_ui_defs.json", &definitions[..]),
        ("ui/extension_surface.json", document.as_bytes()),
    ])
    .map_err(|error| error.to_string())?
    .with_paint_transforms();
    if catalog.diagnostics().is_empty() {
        Ok(catalog)
    } else {
        Err(catalog.diagnostics().join("; "))
    }
}

/// Whether each child of `root` lays out the same alone as beside its siblings, and the root
/// itself draws nothing that every part would repeat. Descendants must stay on their
/// subtree's layer so cached fragments preserve global paint order.
fn splittable(root: &Value, children: &[Value]) -> bool {
    let kind = root.get("type").and_then(Value::as_str);
    children.len() > 1
        && matches!(kind, Some("panel" | "screen" | "input_panel"))
        && !measured_from(root.get("size"), &["%c", "%sm"])
        && children.iter().all(|child| {
            child
                .as_object()
                .and_then(|named| named.values().next())
                .is_some_and(|body| {
                    !measured_from(body.get("size"), &["%sm"]) && flat_descendant_layers(body)
                })
        })
}

/// Whether descendants stay on their parent's layer without instancing grid templates.
/// Grids and layered subtrees stay whole because replay cannot interleave cached draws.
fn flat_descendant_layers(body: &Value) -> bool {
    body.get("type").and_then(Value::as_str) != Some("grid")
        && children(body).all(|child| {
            child.get("layer").and_then(Value::as_i64).unwrap_or(0) == 0
                && flat_descendant_layers(child)
        })
}

/// Whether a size names a unit such as children (`%c`, `%cm`) or siblings (`%sm`).
fn measured_from(size: Option<&Value>, units: &[&str]) -> bool {
    let axis = |value: &Value| {
        value
            .as_str()
            .is_some_and(|text| units.iter().any(|unit| text.contains(unit)))
    };
    match size {
        Some(Value::Array(axes)) => axes.iter().any(axis),
        Some(value) => axis(value),
        None => false,
    }
}

/// The literal draw layer of a named child, for top-level paint order.
fn layer(child: &Value) -> i64 {
    child
        .as_object()
        .and_then(|named| named.values().next())
        .and_then(|body| body.get("layer"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

/// A projected binding the top-level control `body` alone reads as its `#offset`, with an
/// optional companion it alone reads as its `#visible`.
fn floating(surface: &Surface, part: &Value, body: &Value) -> Option<Floating> {
    let bindings = body.get("bindings")?.as_array()?;
    /// The name a binding entry binds to `property`.
    fn target<'a>(binding: &'a Value, property: &str) -> Option<&'a str> {
        (binding.get("binding_name_override").and_then(Value::as_str) == Some(property))
            .then(|| binding.get("binding_name").and_then(Value::as_str))
            .flatten()
    }
    let offset = bindings
        .iter()
        .find_map(|binding| target(binding, "#offset"))
        .filter(|name| {
            surface
                .bindings
                .get(*name)
                .is_some_and(|value| value.projection().is_some())
        })?;
    let companion = projected_visible_name(offset);
    let visible = bindings
        .iter()
        .find_map(|binding| target(binding, "#visible"))
        .filter(|name| *name == companion);
    let mut uses = BTreeMap::new();
    count(part, &mut uses);
    let once = |name: &str| uses.get(name).copied().unwrap_or(0) == 1;
    if !once(offset) || uses.contains_key(&companion) && visible.is_none() {
        return None;
    }
    if visible.is_some() && !once(&companion) {
        return None;
    }
    Some(Floating {
        offset: offset.to_owned(),
        visible: visible.map(str::to_owned),
    })
}

/// Counts every binding-name reference under `value`.
fn count(value: &Value, uses: &mut BTreeMap<String, usize>) {
    for binding in value
        .get("bindings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(name) = binding.get("binding_name").and_then(Value::as_str) {
            for property in json_ui::expression_properties(name) {
                *uses.entry(property).or_default() += 1;
            }
        }
    }
    for body in children(value) {
        count(body, uses);
    }
}

/// Every named child body of `value`.
fn children(value: &Value) -> impl Iterator<Item = &Value> {
    value
        .get("controls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .flat_map(Map::values)
}

/// Adds the global bindings and collections `value` reads, following native grid templates.
fn reads(
    value: &Value,
    document: &Value,
    bindings: &mut BTreeSet<String>,
    collections: &mut BTreeSet<String>,
) {
    for binding in value
        .get("bindings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if binding.get("binding_type").and_then(Value::as_str) == Some("collection") {
            continue;
        }
        if let Some(name) = binding.get("binding_name").and_then(Value::as_str) {
            bindings.extend(json_ui::expression_properties(name));
        }
    }
    if let Some(name) = value.get("collection_name").and_then(Value::as_str)
        && collections.insert(name.to_owned())
        && let Some(template) = value
            .get("grid_item_template")
            .and_then(Value::as_str)
            .and_then(|reference| reference.split_once('.'))
            .map(|(_, local)| &document[local])
    {
        reads(template, document, bindings, collections);
    }
    for body in children(value) {
        reads(body, document, bindings, collections);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A HUD surface with `root` and the given bindings.
    fn surface(root: Value, bindings: Value) -> Surface {
        serde_json::from_value(json!({
            "screen":"hud.root",
            "document":json!({"namespace":"hud","root":root}).to_string(),
            "bindings":bindings,
        }))
        .unwrap()
    }

    /// A label bound to `name`.
    fn label(name: &str) -> Value {
        json!({"type":"label","text":name,"bindings":[{"binding_name":name}]})
    }

    #[test]
    fn panel_roots_split_into_layer_ordered_parts_with_their_own_reads() {
        let surface = surface(
            json!({"type":"panel","size":["100%","100%"],"controls":[
                {"fps":label("#fps")},
                {"back":{"type":"panel","layer":-1,"controls":[{"cps":label("#cps")}],
                    "bindings":[{"binding_name":"(#shown and #enabled)",
                        "binding_name_override":"#visible"}]}},
                {"tag":{"type":"panel","size":[0,0],"bindings":[
                    {"binding_name":"#tag","binding_name_override":"#offset"},
                    {"binding_name":"#tag_visible","binding_name_override":"#visible"}],
                    "controls":[{"text":label("#tag_text")}]}}
            ]}),
            json!({"#fps":"60","#tag":{"project":{"world":[0,64,0]}}}),
        );
        let document = Document::build(&surface).unwrap();
        let reads: Vec<Vec<&str>> = document
            .parts
            .iter()
            .map(|part| part.bindings.iter().map(String::as_str).collect())
            .collect();
        assert_eq!(
            reads,
            [
                vec!["#cps", "#enabled", "#shown"],
                vec!["#fps"],
                vec!["#tag", "#tag_text", "#tag_visible"]
            ]
        );
        assert_eq!(
            document.parts[2].floating,
            Some(Floating {
                offset: "#tag".into(),
                visible: Some("#tag_visible".into())
            })
        );
        assert!(
            document.parts[..2]
                .iter()
                .all(|part| part.floating.is_none())
        );
    }

    #[test]
    fn interdependent_or_drawing_roots_stay_one_part() {
        let two = || json!([{"a":label("#a")},{"b":label("#b")}]);
        for root in [
            json!({"type":"stack_panel","controls":two()}),
            json!({"type":"panel","size":["100%c","100%"],"controls":two()}),
            json!({"type":"panel","controls":[{"a":{"type":"panel","size":["100%sm",10]}},
                {"b":label("#b")}]}),
            json!({"type":"panel","controls":[{"a":label("#a")}]}),
        ] {
            let document = Document::build(&surface(root.clone(), json!({}))).unwrap();
            assert_eq!(document.parts.len(), 1, "{root}");
            assert_eq!(document.parts[0].reference, "hud.root");
        }
    }

    #[test]
    fn projections_read_elsewhere_in_their_part_do_not_float() {
        let surface = surface(
            json!({"type":"panel","controls":[
                {"tag":{"type":"panel","bindings":[
                    {"binding_name":"#tag","binding_name_override":"#offset"}],
                    "controls":[{"x":{"type":"panel","bindings":[
                        {"binding_name":"#tag","binding_name_override":"#offset"}]}}]}},
                {"other":label("#other")}
            ]}),
            json!({"#tag":{"project":{"world":[0,0,0]}}}),
        );
        let document = Document::build(&surface).unwrap();
        assert!(document.parts.iter().all(|part| part.floating.is_none()));
    }

    #[test]
    fn named_controls_embed_with_their_own_reads() {
        let surface = surface(
            json!({"type":"panel","controls":[{"g@hud.card":{}}]}),
            json!({}),
        );
        let mut document: Value = serde_json::from_str(&surface.document).unwrap();
        document["card"] = json!({"type":"panel","size":[20,10],"controls":[
            {"value":label("#card_value")}]});
        let surface = Surface {
            document: document.to_string(),
            ..surface
        };
        let definitions = Definitions::parse(&surface.document).unwrap();
        for definitions in [None, Some(&definitions)] {
            let (_, part) = Document::embedded(&surface, definitions, "card").unwrap();
            assert_eq!(part.reference, "hud.card");
            assert!(part.bindings.contains("#card_value"));
            assert!(Document::embedded(&surface, definitions, "missing").is_err());
        }
    }
}
