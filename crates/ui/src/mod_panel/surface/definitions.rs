//! A surface document's top-level control definitions: replacing individual definitions, and
//! which definitions a control's resolution reads.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use super::static_templates::local_reference;

/// One authored top-level definition.
#[derive(Clone, Debug, PartialEq)]
pub struct Definition {
    /// The document key: the local name with an optional `@` base.
    pub key: String,
    /// The local definition it inherits from.
    pub base: Option<String>,
    pub body: Value,
}

/// A document's namespace and its top-level definitions by local name.
#[derive(Clone, Debug, PartialEq)]
pub struct Definitions {
    pub namespace: String,
    pub definitions: BTreeMap<String, Definition>,
}

impl Definitions {
    /// Reads every top-level definition of `document`, keyed by local name.
    pub fn parse(document: &str) -> Result<Self, String> {
        let Value::Object(object) = serde_json::from_str(document)
            .map_err(|error| format!("invalid surface JSON: {error}"))?
        else {
            return Err("surface document must be an object".into());
        };
        let namespace = object
            .get("namespace")
            .and_then(Value::as_str)
            .ok_or("surface document must contain its own namespace")?
            .to_owned();
        let mut definitions = BTreeMap::new();
        for (key, body) in object {
            if key == "namespace" {
                continue;
            }
            let (local, base) = local_reference(&key, &namespace)?;
            if definitions
                .insert(local, Definition { key, base, body })
                .is_some()
            {
                return Err("duplicate surface definition".into());
            }
        }
        Ok(Self {
            namespace,
            definitions,
        })
    }

    /// Document text holding the named definitions as authored, plus `extra` under its key.
    pub fn document<'a>(
        &self,
        names: impl IntoIterator<Item = &'a String>,
        extra: Option<(&str, &Value)>,
    ) -> String {
        let mut object = Map::new();
        object.insert("namespace".into(), Value::String(self.namespace.clone()));
        for definition in names
            .into_iter()
            .filter_map(|name| self.definitions.get(name))
        {
            object.insert(definition.key.clone(), definition.body.clone());
        }
        if let Some((key, body)) = extra {
            object.insert(key.into(), body.clone());
        }
        Value::Object(object).to_string()
    }

    /// Local names defined differently in `next`, including added and removed definitions.
    pub fn changed(&self, next: &Self) -> BTreeSet<String> {
        let mut changed: BTreeSet<String> = self
            .definitions
            .iter()
            .filter(|(name, definition)| next.definitions.get(*name) != Some(*definition))
            .map(|(name, _)| name.clone())
            .collect();
        changed.extend(
            next.definitions
                .keys()
                .filter(|name| !self.definitions.contains_key(*name))
                .cloned(),
        );
        if self.namespace != next.namespace {
            changed.extend(self.definitions.keys().cloned());
            changed.extend(next.definitions.keys().cloned());
        }
        changed
    }

    /// The definitions an authored control reads directly: those its own children inherit
    /// from, at any depth.
    pub fn references(&self, control: &Value) -> BTreeSet<String> {
        let mut references = BTreeSet::new();
        self.collect(control, &mut references);
        references
    }

    /// Adds the bases of every named child under `control`.
    fn collect(&self, control: &Value, references: &mut BTreeSet<String>) {
        for child in control
            .get("controls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
        {
            for (key, body) in child {
                if let Ok((_, Some(base))) = local_reference(key, &self.namespace) {
                    references.insert(base);
                }
                self.collect(body, references);
            }
        }
    }

    /// Every definition reading `name` resolves, `name` included: its base chain and the
    /// definitions its children inherit from, transitively.
    pub fn reads(&self, name: &str) -> BTreeSet<String> {
        let mut reads = BTreeSet::new();
        let mut pending = vec![name.to_owned()];
        while let Some(name) = pending.pop() {
            if !reads.insert(name.clone()) {
                continue;
            }
            if let Some(definition) = self.definitions.get(&name) {
                pending.extend(definition.base.iter().cloned());
                pending.extend(self.references(&definition.body));
            }
        }
        reads
    }

    /// Every definition whose resolution reads one of `changed`: the changed definitions and
    /// those inheriting from or instancing them, transitively.
    pub fn affected(&self, changed: &BTreeSet<String>) -> BTreeSet<String> {
        let mut readers: BTreeMap<String, Vec<&str>> = BTreeMap::new();
        for (name, definition) in &self.definitions {
            for read in definition
                .base
                .iter()
                .cloned()
                .chain(self.references(&definition.body))
            {
                readers.entry(read).or_default().push(name);
            }
        }
        let mut affected = BTreeSet::new();
        let mut pending: Vec<String> = changed.iter().cloned().collect();
        while let Some(name) = pending.pop() {
            if let Some(names) = readers.get(&name) {
                pending.extend(
                    names
                        .iter()
                        .filter(|reader| !affected.contains(**reader))
                        .map(|reader| (*reader).to_owned()),
                );
            }
            affected.insert(name);
        }
        affected
    }
}

/// Replaces, adds (`Some`) or removes (`None`) top-level definitions of `document`, each named
/// by its local name with an optional `@` base, as a document key is. Validation of the result
/// is the caller's.
pub(super) fn patch(
    document: &str,
    controls: &BTreeMap<String, Option<Value>>,
) -> Result<String, String> {
    let Value::Object(mut object) =
        serde_json::from_str(document).map_err(|error| format!("invalid surface JSON: {error}"))?
    else {
        return Err("surface document must be an object".into());
    };
    let namespace = object
        .get("namespace")
        .and_then(Value::as_str)
        .ok_or("surface document must contain its own namespace")?
        .to_owned();
    let mut keys = BTreeMap::new();
    for key in object.keys().filter(|key| *key != "namespace") {
        keys.insert(local_reference(key, &namespace)?.0, key.clone());
    }
    for edit in edits(&namespace, |local| keys.contains_key(local), controls)? {
        if let Some(previous) = keys.remove(&edit.local) {
            object.remove(&previous);
        }
        if let Some(body) = edit.body {
            object.insert(edit.key.to_owned(), body.clone());
        }
    }
    Ok(Value::Object(object).to_string())
}

/// One checked change to a top-level definition.
pub(super) struct Edit<'a> {
    /// The definition's local name.
    pub local: String,
    /// Its new document key, the local name with an optional `@` base.
    pub key: &'a str,
    /// Its new body, or none to remove it.
    pub body: Option<&'a Value>,
}

/// Checks `controls`, in key order, as replacements (an object) or removals (`null`) of the
/// local definitions of `namespace`, where `exists` names the current ones.
pub(super) fn edits<'a>(
    namespace: &str,
    exists: impl Fn(&str) -> bool,
    controls: &'a BTreeMap<String, Option<Value>>,
) -> Result<Vec<Edit<'a>>, String> {
    let mut seen = BTreeSet::new();
    let mut edits = Vec::with_capacity(controls.len());
    for (key, control) in controls {
        if key == "namespace" {
            return Err("surface update cannot replace the namespace".into());
        }
        let (local, _) = local_reference(key, namespace)?;
        if local.contains('.') {
            return Err("surface update control names carry no namespace".into());
        }
        if !seen.insert(local.clone()) {
            return Err("surface update repeats a control".into());
        }
        match control {
            Some(control) if !control.is_object() => {
                return Err("surface update control must be an object".into());
            }
            None if !exists(&local) => {
                return Err("surface update removes an unknown control".into());
            }
            _ => {}
        }
        edits.push(Edit {
            local,
            key,
            body: control.as_ref(),
        });
    }
    Ok(edits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn definitions(document: Value) -> Definitions {
        Definitions::parse(&document.to_string()).unwrap()
    }

    #[test]
    fn patches_replace_add_and_remove_definitions_by_local_name() {
        let document = json!({"namespace":"hud","root":{"type":"panel"},
            "card@hud.base":{"size":[1,1]},"base":{"type":"panel"},"old":{"type":"panel"}})
        .to_string();
        let patched = patch(
            &document,
            &[
                ("card".to_owned(), Some(json!({"type":"label"}))),
                ("new@base".to_owned(), Some(json!({}))),
                ("old".to_owned(), None),
            ]
            .into(),
        )
        .unwrap();
        let patched: Value = serde_json::from_str(&patched).unwrap();
        assert_eq!(patched["card"], json!({"type":"label"}));
        assert!(patched.get("card@hud.base").is_none(), "the base goes too");
        assert_eq!(patched["new@base"], json!({}));
        assert!(patched.get("old").is_none());
        assert_eq!(patched["root"], json!({"type":"panel"}));
        for controls in [
            [("missing".to_owned(), None)].into(),
            [("card".to_owned(), Some(json!([])))].into(),
            [("namespace".to_owned(), Some(json!({})))].into(),
            [("other.card".to_owned(), Some(json!({})))].into(),
            [("x@foreign.base".to_owned(), Some(json!({})))].into(),
            [
                ("card".to_owned(), Some(json!({}))),
                ("card@base".to_owned(), Some(json!({}))),
            ]
            .into(),
        ] {
            assert!(patch(&document, &controls).is_err());
        }
    }

    #[test]
    fn affected_definitions_follow_inheritance_and_instances() {
        let old = definitions(json!({"namespace":"hud",
            "root":{"type":"panel","controls":[{"a@hud.card":{}},{"b":{"type":"label"}}]},
            "card@hud.frame":{"controls":[{"fill@hud.fill":{}}]},
            "frame":{"type":"panel"},"fill":{"type":"image"},"preview":{"type":"panel"}}));
        let mut next = old.clone();
        next.definitions.get_mut("fill").unwrap().body = json!({"type":"label"});
        let changed = old.changed(&next);
        assert_eq!(changed, ["fill".to_owned()].into());
        assert_eq!(
            next.affected(&changed),
            ["card", "fill", "root"].map(str::to_owned).into()
        );
        assert_eq!(
            next.reads("card"),
            ["card", "fill", "frame"].map(str::to_owned).into()
        );
        let root = &next.definitions["root"].body;
        assert_eq!(
            next.references(&root["controls"][0]["a@hud.card"]),
            BTreeSet::new()
        );
        assert_eq!(next.references(root), ["card".to_owned()].into());
        next.definitions.remove("preview");
        assert!(old.changed(&next).contains("preview"));
    }
}
