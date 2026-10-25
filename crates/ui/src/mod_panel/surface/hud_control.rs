//! Panel nodes that embed a named control of the same mod's HUD surface document.

use serde_json::{Map, Value};

use super::{binding_name, identifier};

/// Draws the control named by `control`, or by the text value of `control_binding`, from the
/// owning mod's HUD surface, with the HUD surface's bindings, centred in the node. An unknown
/// name draws nothing. [`Fitting`] describes how the optional `scale`, `fit` and `max_scale`
/// size it.
pub const RENDERER: &str = "cinnabar_hud_control";
/// The least `scale` and `max_scale`, a multiple of the control's HUD size, as for HUD cards.
pub const MIN_SCALE: f32 = crate::mod_hud::MIN_CARD_SCALE;
/// The greatest `scale` and `max_scale`, and the default `max_scale`.
pub const MAX_SCALE: f32 = crate::mod_hud::MAX_CARD_SCALE;
/// The `scale` property and the binding override that supersedes it.
const SCALE: [&str; 2] = ["scale", "#scale"];
/// The `fit` property and the binding override that supersedes it.
const FIT: [&str; 2] = ["fit", "#fit"];
/// The `max_scale` property and the binding override that supersedes it.
const FIT_CAP: [&str; 2] = ["max_scale", "#max_scale"];
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

/// Requires a literal local control name or a binding name, never both, and fitting values
/// within the bounds [`Fitting::read`] applies to bound values when drawing.
pub(super) fn validate(object: &Map<String, Value>) -> Result<(), String> {
    let control = object.get(CONTROL);
    let binding = object.get(CONTROL_BINDING);
    match (control, binding) {
        (Some(name), None) if name.as_str().is_some_and(control_name) => {}
        (None, Some(name)) if name.as_str().is_some_and(binding_name) => {}
        _ => {
            return Err(format!(
                "{RENDERER} requires one local `{CONTROL}` name or `{CONTROL_BINDING}` binding"
            ));
        }
    }
    for name in SCALE.iter().chain(&FIT_CAP) {
        if object
            .get(*name)
            .is_some_and(|value| scale(value).is_none())
        {
            return Err(format!(
                "{RENDERER} {name} must be a number from {MIN_SCALE} to {MAX_SCALE}"
            ));
        }
    }
    for name in FIT {
        if object.get(name).is_some_and(|value| fit(value).is_none()) {
            return Err(format!("{RENDERER} {name} must be a fraction in (0, 1]"));
        }
    }
    Ok(())
}

/// How a node sizes its embedded control, always centred in the node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fitting {
    /// Neither `fit` nor `scale`: the control's laid-out box fitted uniformly to the node, so
    /// authored padding frames it.
    Layout,
    /// `scale` without `fit`: the laid-out box at this multiple of the control's HUD size.
    Scale(f32),
    /// `fit`: the control's painted ink fills `fraction` of the node along its limiting axis,
    /// at most `max_scale` times its HUD size. `scale` is then unused.
    Ink { fraction: f32, max_scale: f32 },
}

impl Fitting {
    /// Reads a node's fitting through `get`, each binding override before its literal. None
    /// when a present value is outside its bounds, which draws nothing.
    pub fn read<'a>(get: impl Fn(&str) -> Option<&'a Value>) -> Option<Self> {
        let value = |[literal, bound]: [&str; 2]| get(bound).or_else(|| get(literal));
        let scale_value = value(SCALE).map(scale);
        let cap = value(FIT_CAP).map(scale);
        if scale_value == Some(None) || cap == Some(None) {
            return None;
        }
        Some(match value(FIT).map(fit) {
            Some(fraction) => Self::Ink {
                fraction: fraction?,
                max_scale: cap.flatten().unwrap_or(MAX_SCALE),
            },
            None => scale_value.flatten().map_or(Self::Layout, Self::Scale),
        })
    }
}

/// A finite multiple of the control's HUD size within [`MIN_SCALE`] and [`MAX_SCALE`].
fn scale(value: &Value) -> Option<f32> {
    value
        .as_f64()
        .filter(|scale| {
            scale.is_finite() && (f64::from(MIN_SCALE)..=f64::from(MAX_SCALE)).contains(scale)
        })
        .map(|scale| scale as f32)
}

/// A fraction of the node in (0, 1].
fn fit(value: &Value) -> Option<f32> {
    value
        .as_f64()
        .filter(|fit| *fit > 0. && *fit <= 1.)
        .map(|fit| fit as f32)
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

    #[test]
    fn hud_control_fitting_literals_and_bound_overrides_stay_within_bounds() {
        for fitting in [
            json!({"fit":0.78,"max_scale":2.4}),
            json!({"fit":1,"max_scale":MAX_SCALE}),
            json!({"fit":0.001,"max_scale":MIN_SCALE}),
            json!({"scale":MIN_SCALE}),
            json!({"scale":MAX_SCALE,"max_scale":2}),
            json!({"#fit":0.5,"#max_scale":3,"#scale":1}),
            json!({"bindings":[
                {"binding_name":"#item_fit","binding_name_override":"#fit"},
                {"binding_name":"#item_cap","binding_name_override":"#max_scale"},
                {"binding_name":"#item_scale","binding_name_override":"#scale"}
            ]}),
        ] {
            let mut node = fitting.clone();
            node["control"] = json!("armor");
            assert!(validate(node.as_object().unwrap()).is_ok(), "{node}");
        }
        for fitting in [
            json!({"fit":0}),
            json!({"fit":-0.1}),
            json!({"fit":1.001}),
            json!({"fit":"0.78"}),
            json!({"fit":null}),
            json!({"#fit":2}),
            json!({"max_scale":MIN_SCALE / 2.}),
            json!({"max_scale":MAX_SCALE + 1.}),
            json!({"max_scale":"2.4"}),
            json!({"max_scale":null}),
            json!({"#max_scale":0}),
            json!({"scale":0}),
            json!({"scale":6}),
            json!({"#scale":-1}),
            json!({"fit":0.78,"scale":MIN_SCALE / 2.}),
        ] {
            let mut node = fitting.clone();
            node["control"] = json!("armor");
            assert!(validate(node.as_object().unwrap()).is_err(), "{node}");
        }
    }

    #[test]
    fn panel_surfaces_admit_only_bounded_hud_control_fitting() {
        let surface = |fitting: Value| {
            let mut node = json!({"type":"custom","renderer":RENDERER,"control":"armor",
                "size":[80,40]});
            node.as_object_mut()
                .unwrap()
                .extend(fitting.as_object().unwrap().clone());
            super::super::Surface {
                screen: "menu.root".into(),
                document: json!({"namespace":"menu","root":{"type":"panel",
                    "controls":[{"preview":node}]}})
                .to_string(),
                bindings: Default::default(),
                transitions: Default::default(),
                collections: Default::default(),
            }
        };
        surface(json!({"fit":0.78,"max_scale":2.4}))
            .validate()
            .unwrap();
        for invalid in [json!({"fit":0}), json!({"#max_scale":6})] {
            let error = surface(invalid.clone()).validate().unwrap_err();
            assert!(error.contains(RENDERER), "{invalid}: {error}");
        }
    }

    #[test]
    fn hud_control_fitting_reads_bound_values_with_the_literal_bounds() {
        let read = |node: Value| Fitting::read(|name| node.get(name));
        assert_eq!(read(json!({})), Some(Fitting::Layout));
        assert_eq!(read(json!({"max_scale":2})), Some(Fitting::Layout));
        assert_eq!(read(json!({"scale":0.5})), Some(Fitting::Scale(0.5)));
        assert_eq!(
            read(json!({"fit":0.78,"scale":2})),
            Some(Fitting::Ink {
                fraction: 0.78,
                max_scale: MAX_SCALE
            })
        );
        assert_eq!(
            read(json!({"fit":0.78,"max_scale":2.4,"#fit":0.5,"#max_scale":3})),
            Some(Fitting::Ink {
                fraction: 0.5,
                max_scale: 3.
            }),
            "bound values supersede literals"
        );
        assert_eq!(
            read(json!({"#scale":2,"scale":1})),
            Some(Fitting::Scale(2.))
        );
        for invalid in [
            json!({"#fit":0}),
            json!({"#fit":1.1}),
            json!({"#fit":null}),
            json!({"fit":0.78,"#max_scale":0.1}),
            json!({"fit":0.78,"#max_scale":6}),
            json!({"#scale":"1"}),
            json!({"fit":0.78,"#scale":0}),
        ] {
            assert_eq!(read(invalid.clone()), None, "{invalid}");
        }
    }
}
