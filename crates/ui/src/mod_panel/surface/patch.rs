//! Sparse binding and collection-order changes to a retained surface.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::{Surface, SurfaceRole, SurfaceValue};

/// Changing values never need to carry the retained document.
pub const MAX_SURFACE_PATCH_BYTES: usize = 64 * 1024;

/// The binding and collection subset of a panel patch: omitted names keep their values.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfacePatch {
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

    /// Returns `current` with this patch applied and revalidated for `role`; errors leave
    /// `current` intact.
    pub fn apply(self, current: &Surface, role: SurfaceRole) -> Result<Surface, String> {
        let mut surface = current.clone();
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
        surface.validate_bindings_as(role)?;
        Ok(surface)
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
}
