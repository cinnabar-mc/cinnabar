//! Panel nodes that embed a named control of the same mod's HUD surface document.

use serde_json::{Map, Value};

use super::{binding_name, identifier};

/// Draws the control named by `control`, or by the text value of `control_binding`, from the
/// owning mod's HUD surface, scaled uniformly to fit and centred in the node, with the HUD
/// surface's bindings. An unknown name draws nothing.
pub const RENDERER: &str = "cinnabar_hud_control";
/// The literal control name property.
pub const CONTROL: &str = "control";
/// The property naming a text binding or collection field that holds the control name.
pub const CONTROL_BINDING: &str = "control_binding";
/// The node property a `control_binding` value is bound to in the canonical document.
pub const BOUND_CONTROL: &str = "#cinnabar_hud_control";

/// Binds each `control_binding` name to [`BOUND_CONTROL`] so the renderer reads its value; an
/// `#item_` name later becomes a collection field of its grid tile.
pub(super) fn bind_selectors(value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        if object.get("renderer").and_then(Value::as_str) == Some(RENDERER)
            && let Some(name) = object.get(CONTROL_BINDING).and_then(Value::as_str)
        {
            let binding = serde_json::json!({"binding_name":name,
                "binding_name_override":BOUND_CONTROL});
            match object.get_mut("bindings").and_then(Value::as_array_mut) {
                Some(bindings) => bindings.push(binding),
                None => {
                    object.insert("bindings".into(), Value::Array(vec![binding]));
                }
            }
        }
        if let Some(children) = object.get_mut("controls").and_then(Value::as_array_mut) {
            for child in children.iter_mut().filter_map(Value::as_object_mut) {
                for body in child.values_mut() {
                    bind_selectors(body);
                }
            }
        }
    }
}

/// Requires a literal local control name or a binding name, never both.
pub(super) fn validate(object: &Map<String, Value>) -> Result<(), String> {
    let control = object.get(CONTROL);
    let binding = object.get(CONTROL_BINDING);
    match (control, binding) {
        (Some(name), None) if name.as_str().is_some_and(control_name) => Ok(()),
        (None, Some(name)) if name.as_str().is_some_and(binding_name) => Ok(()),
        _ => Err(format!(
            "{RENDERER} requires one local `{CONTROL}` name or `{CONTROL_BINDING}` binding"
        )),
    }
}

/// A local definition name in the HUD document, without namespace or inheritance.
pub fn control_name(name: &str) -> bool {
    identifier(name) && !name.contains('.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn control_bindings_bind_the_renderer_selector() {
        let mut node = json!({"type":"panel","controls":[{"preview":{"type":"custom",
            "renderer":RENDERER,"control_binding":"#item_preview",
            "bindings":[{"binding_name":"#shown","binding_name_override":"#visible"}]}}]});
        bind_selectors(&mut node);
        let bindings = &node["controls"][0]["preview"]["bindings"];
        assert_eq!(bindings.as_array().unwrap().len(), 2);
        assert_eq!(bindings[1]["binding_name"], "#item_preview");
        assert_eq!(bindings[1]["binding_name_override"], BOUND_CONTROL);
    }

    #[test]
    fn hud_control_nodes_name_one_local_control_or_one_binding() {
        for node in [
            json!({"control":"armor"}),
            json!({"control_binding":"#selected_module"}),
        ] {
            assert!(validate(node.as_object().unwrap()).is_ok(), "{node}");
        }
        for node in [
            json!({}),
            json!({"control":"armor","control_binding":"#selected"}),
            json!({"control":"other.armor"}),
            json!({"control":""}),
            json!({"control":12}),
            json!({"control_binding":"selected"}),
        ] {
            assert!(validate(node.as_object().unwrap()).is_err(), "{node}");
        }
    }
}
