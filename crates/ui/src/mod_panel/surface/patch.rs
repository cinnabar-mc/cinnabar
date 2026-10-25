//! Sparse binding, collection-order and control-definition changes to a retained surface.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use super::{HudIndex, Surface, SurfaceRole, SurfaceValue};

/// Changing values and a few control definitions never need to carry the whole document.
pub const MAX_SURFACE_PATCH_BYTES: usize = 64 * 1024;

/// The binding and collection subset of a panel patch, plus binding removals and replaced
/// control definitions: omitted names keep their values.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfacePatch {
    /// Replaces (an object) or removes (`null`) top-level control definitions of the
    /// document, keyed by local name without namespace, optionally with an `@` base as a
    /// document key; the patched document is admitted exactly as a whole one would be.
    #[serde(default)]
    pub controls: Option<BTreeMap<String, Option<Value>>>,
    /// Removes the named bindings before `bindings` and `binding_changes` apply. Each must be
    /// bound and named once, and not set by the same patch; at most the role's binding
    /// budget. A removed name the document still reads draws as never bound.
    #[serde(default)]
    pub remove_bindings: Vec<String>,
    /// Replaces every binding.
    #[serde(default)]
    pub bindings: Option<BTreeMap<String, SurfaceValue>>,
    /// Sets the named bindings and keeps the others.
    #[serde(default)]
    pub binding_changes: Option<BTreeMap<String, SurfaceValue>>,
    /// Filters or sorts admitted tile data without resending its immutable metadata.
    #[serde(default)]
    pub collection_order: BTreeMap<String, Vec<usize>>,
}

impl SurfacePatch {
    /// Reads a bounded patch without resolving any retained document.
    pub fn parse(json: &str) -> Result<Self, String> {
        if json.len() > MAX_SURFACE_PATCH_BYTES {
            return Err("surface update exceeds its byte limit".into());
        }
        serde_json::from_str(json).map_err(|error| format!("invalid surface update: {error}"))
    }

    /// Returns `current` with this patch applied and revalidated for `role`, the whole
    /// document when it replaces controls; errors leave `current` intact.
    pub fn apply(self, current: &Surface, role: SurfaceRole) -> Result<Surface, String> {
        let mut surface = current.clone();
        let redefined = self.controls.is_some();
        if let Some(controls) = &self.controls {
            surface.document = super::definitions::patch(&surface.document, controls)?;
        }
        self.apply_values(&mut surface, role)?;
        if redefined {
            surface.validate_as(role)?;
        } else {
            surface.validate_bindings_as(role)?;
        }
        Ok(surface)
    }

    /// [`Self::apply`] for a HUD surface, with the index of `current`'s document when one is
    /// known. Returns the patched surface and the index of its document, with exactly the
    /// result and error `apply` would give: a `controls` patch the index can check revalidates
    /// only the definitions it changes and the root children reading them, and anything else
    /// validates the whole document.
    pub fn apply_hud(
        self,
        current: &Surface,
        index: &HudIndex,
    ) -> Result<(Surface, HudIndex), String> {
        let Some(controls) = &self.controls else {
            let kept = if index.describes(current) {
                index.clone()
            } else {
                HudIndex::default()
            };
            return Ok((self.apply(current, SurfaceRole::Hud)?, kept));
        };
        if let Some((document, next)) = index.patched(current, controls) {
            let mut surface = Surface {
                screen: current.screen.clone(),
                document,
                bindings: current.bindings.clone(),
                transitions: current.transitions.clone(),
                collections: current.collections.clone(),
            };
            self.apply_values(&mut surface, SurfaceRole::Hud)?;
            surface.validate_bindings_as(SurfaceRole::Hud)?;
            return Ok((surface, next));
        }
        let mut surface = current.clone();
        surface.document = super::definitions::patch(&surface.document, controls)?;
        self.apply_values(&mut surface, SurfaceRole::Hud)?;
        let index = HudIndex::new(&surface)?;
        Ok((surface, index))
    }

    /// Applies this patch's binding removals, bindings, binding changes and collection orders
    /// to `surface`, in that order, with `role`'s bound on removals.
    fn apply_values(self, surface: &mut Surface, role: SurfaceRole) -> Result<(), String> {
        if self.remove_bindings.len() > role.max_bindings() {
            return Err("surface update removes too many bindings".into());
        }
        for name in &self.remove_bindings {
            let set = |bindings: &Option<BTreeMap<String, SurfaceValue>>| {
                bindings
                    .as_ref()
                    .is_some_and(|bindings| bindings.contains_key(name))
            };
            if set(&self.bindings) || set(&self.binding_changes) {
                return Err("surface update both sets and removes a binding".into());
            }
            if surface.bindings.remove(name).is_none() {
                return Err("surface update removes an unbound binding".into());
            }
        }
        if let Some(bindings) = self.bindings {
            surface.bindings = bindings;
        }
        if let Some(changes) = self.binding_changes {
            surface.bindings.extend(changes);
        }
        for (name, order) in self.collection_order {
            surface
                .collections
                .get_mut(&name)
                .ok_or("surface update names an unknown collection")?
                .order = order;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A one-label HUD surface with `bindings`.
    fn surface(bindings: serde_json::Value) -> Surface {
        serde_json::from_value(json!({
            "screen":"hud.root",
            "document":json!({"namespace":"hud","root":{"type":"panel","controls":[
                {"fps":{"type":"label","text":"#fps","bindings":[{"binding_name":"#fps"}]}}
            ]}}).to_string(),
            "bindings":bindings,
        }))
        .unwrap()
    }

    #[test]
    fn surface_patches_replace_or_merge_bindings_and_keep_omitted_values() {
        let current = surface(json!({"#fps":"60","#cps":"0"}));
        let merged = SurfacePatch::parse(r##"{"binding_changes":{"#fps":"61"}}"##)
            .unwrap()
            .apply(&current, SurfaceRole::Hud)
            .unwrap();
        assert_eq!(
            merged.bindings["#fps"],
            SurfaceValue::Text("61".into()),
            "changed value"
        );
        assert_eq!(merged.bindings["#cps"], SurfaceValue::Text("0".into()));
        let replaced = SurfacePatch::parse(r##"{"bindings":{"#fps":"1"}}"##)
            .unwrap()
            .apply(&current, SurfaceRole::Hud)
            .unwrap();
        assert_eq!(replaced.bindings.len(), 1);
        assert_eq!(replaced.document, current.document);
    }

    #[test]
    fn surface_patches_are_bounded_and_validated_for_their_role() {
        assert!(SurfacePatch::parse(&" ".repeat(MAX_SURFACE_PATCH_BYTES + 1)).is_err());
        assert!(SurfacePatch::parse(r#"{"document":"{}"}"#).is_err());
        let current = surface(json!({}));
        let many = |count: usize| {
            let changes: BTreeMap<_, _> = (0..count)
                .map(|i| (format!("#row_{i}"), SurfaceValue::Number(i as f64)))
                .collect();
            SurfacePatch {
                binding_changes: Some(changes),
                ..Default::default()
            }
        };
        assert!(
            many(super::super::MAX_HUD_SURFACE_BINDINGS)
                .apply(&current, SurfaceRole::Hud)
                .is_ok()
        );
        assert!(
            many(super::super::MAX_HUD_SURFACE_BINDINGS + 1)
                .apply(&current, SurfaceRole::Hud)
                .is_err()
        );
        assert!(
            many(super::super::MAX_SURFACE_BINDINGS + 1)
                .apply(&current, SurfaceRole::Panel)
                .is_err()
        );
        assert!(
            SurfacePatch::parse(r#"{"collection_order":{"missing":[0]}}"#)
                .unwrap()
                .apply(&current, SurfaceRole::Hud)
                .is_err()
        );
        let projected = SurfacePatch::parse(
            r##"{"binding_changes":{"#tag":{"project":{"world":[0,0,0]}},"#tag_visible":true}}"##,
        )
        .unwrap();
        assert!(projected.apply(&current, SurfaceRole::Hud).is_err());
    }

    #[test]
    fn binding_removals_apply_before_binding_changes_and_name_only_bound_names() {
        let current = surface(json!({"#fps":"60","#cps":"0"}));
        let apply = |patch: serde_json::Value| {
            SurfacePatch::parse(&patch.to_string())
                .and_then(|patch| patch.apply(&current, SurfaceRole::Hud))
        };
        let names = |surface: &Surface| surface.bindings.keys().cloned().collect::<Vec<_>>();
        let removed = apply(json!({"remove_bindings":["#fps"]})).unwrap();
        assert_eq!(names(&removed), ["#cps"]);
        assert_eq!(
            removed.document, current.document,
            "the document still reads the removed name, as it may read any unbound one"
        );
        let changed =
            apply(json!({"remove_bindings":["#fps"],"binding_changes":{"#rate":"1"}})).unwrap();
        assert_eq!(names(&changed), ["#cps", "#rate"]);
        let replaced = apply(json!({"remove_bindings":["#cps"],"bindings":{"#fps":"1"}})).unwrap();
        assert_eq!(names(&replaced), ["#fps"]);
        assert_eq!(apply(json!({"remove_bindings":[]})).unwrap(), current);
        for (invalid, error) in [
            (json!({"remove_bindings":["#missing"]}), "unbound"),
            (json!({"remove_bindings":["fps"]}), "unbound"),
            (json!({"remove_bindings":["#fps","#fps"]}), "unbound"),
            (
                json!({"remove_bindings":["#fps"],"binding_changes":{"#fps":"1"}}),
                "both sets and removes",
            ),
            (
                json!({"remove_bindings":["#fps"],"bindings":{"#fps":"1"}}),
                "both sets and removes",
            ),
        ] {
            let result = apply(invalid.clone());
            assert!(
                result.as_ref().is_err_and(|e| e.contains(error)),
                "{invalid}: {result:?}"
            );
        }
        assert!(SurfacePatch::parse(r##"{"remove_bindings":"#fps"}"##).is_err());
        let projected = surface(json!({"#tag":{"project":{"world":[0,0,0]}}}));
        let companion = json!({"binding_changes":{"#tag_visible":true}});
        let mut freed = companion.clone();
        freed["remove_bindings"] = json!(["#tag"]);
        assert!(
            SurfacePatch::parse(&companion.to_string())
                .and_then(|patch| patch.apply(&projected, SurfaceRole::Hud))
                .is_err()
        );
        assert_eq!(
            names(
                &SurfacePatch::parse(&freed.to_string())
                    .and_then(|patch| patch.apply(&projected, SurfaceRole::Hud))
                    .unwrap()
            ),
            ["#tag_visible"],
            "removing a projection first frees its companion's name"
        );
    }

    #[test]
    fn binding_removals_are_bounded_by_the_role_binding_budget() {
        let full: serde_json::Map<_, _> = (0..super::super::MAX_HUD_SURFACE_BINDINGS)
            .map(|i| (format!("#row_{i}"), json!(i)))
            .collect();
        let current = surface(serde_json::Value::Object(full.clone()));
        let removing = |names: Vec<String>, role| {
            SurfacePatch {
                remove_bindings: names,
                ..Default::default()
            }
            .apply(&current, role)
        };
        let every: Vec<String> = full.keys().cloned().collect();
        assert!(
            removing(every.clone(), SurfaceRole::Hud)
                .unwrap()
                .bindings
                .is_empty()
        );
        let mut excess = every.clone();
        excess.push(every[0].clone());
        assert_eq!(
            removing(excess, SurfaceRole::Hud).unwrap_err(),
            "surface update removes too many bindings"
        );
        assert_eq!(
            removing(
                every[..super::super::MAX_SURFACE_BINDINGS + 1].to_vec(),
                SurfaceRole::Panel
            )
            .unwrap_err(),
            "surface update removes too many bindings"
        );
    }

    /// Applies a `controls` patch to `current` as a HUD surface.
    fn redefine(current: &Surface, controls: serde_json::Value) -> Result<Surface, String> {
        SurfacePatch::parse(&json!({"controls":controls}).to_string())
            .and_then(|patch| patch.apply(current, SurfaceRole::Hud))
    }

    #[test]
    fn control_patches_replace_add_and_remove_definitions_and_keep_the_rest() {
        let current = surface(json!({"#fps":"60"}));
        let next = redefine(
            &current,
            json!({"card":{"type":"panel","size":[20,10]},"root":{"type":"panel","controls":[
                {"fps":{"type":"label","text":"#fps","bindings":[{"binding_name":"#fps"}]}},
                {"card@hud.card":{}}]}}),
        )
        .unwrap();
        let document: serde_json::Value = serde_json::from_str(&next.document).unwrap();
        assert_eq!(document["card"]["size"], json!([20, 10]));
        assert_eq!(document["namespace"], "hud");
        assert_eq!(next.bindings, current.bindings);
        let removed = redefine(
            &next,
            json!({"card":null,"root":{"type":"panel","controls":[]}}),
        )
        .unwrap();
        let document: serde_json::Value = serde_json::from_str(&removed.document).unwrap();
        assert!(document.get("card").is_none());
        assert_eq!(
            SurfacePatch::parse(r#"{"controls":{}}"#)
                .unwrap()
                .apply(&current, SurfaceRole::Hud)
                .unwrap()
                .document
                .len(),
            current.document.len(),
            "an empty patch keeps every definition"
        );
    }

    #[test]
    fn control_patches_validate_the_whole_patched_document() {
        let current = surface(json!({}));
        for controls in [
            json!({"root":null}),
            json!({"root":{"type":"panel","controls":[{"x@hud.missing":{}}]}}),
            json!({"a":{"type":"panel","controls":[{"b@hud.b":{}}]},
                "b":{"type":"panel","controls":[{"a@hud.a":{}}]},
                "root":{"type":"panel","controls":[{"a@hud.a":{}}]}}),
            json!({"root":{"type":"custom","renderer":"unknown"}}),
            json!({"root":{"type":"panel","controls":[{"p":{"type":"custom",
                "renderer":"cinnabar_hud_control","control":"root"}}]}}),
            json!({"root":{"type":"button","button_mappings":[{"from_button_id":
                "button.menu_select","mapping_type":"pressed","to_button_id":"mod.control:0"}]}}),
            json!({"card":{"type":"label","text":"x".repeat(MAX_SURFACE_PATCH_BYTES)}}),
        ] {
            assert!(redefine(&current, controls.clone()).is_err(), "{controls}");
        }
        let big = (0..48)
            .map(|i| {
                (
                    format!("card{i}"),
                    json!({"type":"label","text":"x".repeat(5000)}),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        let mut grown = current.clone();
        for chunk in big.into_iter().collect::<Vec<_>>().chunks(12) {
            let controls: serde_json::Map<_, _> = chunk.iter().cloned().collect();
            match redefine(&grown, serde_json::Value::Object(controls)) {
                Ok(next) => grown = next,
                Err(error) => {
                    assert!(error.contains("byte limit"), "{error}");
                    return;
                }
            }
        }
        panic!("the patched document must stay within its byte limit");
    }
}
