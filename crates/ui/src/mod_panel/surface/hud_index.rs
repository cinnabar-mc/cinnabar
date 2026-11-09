//! What whole-document validation established about an admitted HUD surface document, so a
//! control patch revalidates only the definitions it changes and the root children reading them.
//!
//! A root child's resolution reads only the root's own properties, the child's inline body and
//! the definitions its literal `@` bases reach, unless substitution produces an animation
//! reference: an animated property whose parameter names `@` another definition. Without an `@`
//! in any string value, each root child resolves the same alone as beside its siblings while the
//! whole tree stays below the node budget, so its resolved node count and validity are cached and
//! a patch resolves only the children whose literal reads it changes, each against the
//! definitions it reads. The authored node total, inheritance cycles, expanded property bytes
//! and document bytes are rechecked from the changed definitions and the definitions reading
//! them.
//!
//! Whole-document validation stays the reference. It decides, and words the error of, every
//! patch the index cannot prove valid: any rejection; a patch to the root's own properties or
//! base; and any document with collections, a root base, an `@` in a string value, or a resolved
//! tree at the node budget, none of which is indexed.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use serde_json::{Map, Value};

use super::definitions::edits;
use super::static_templates::{
    acyclic, authored, definition_bytes, largest_parameter, local_reference,
};
use super::{MAX_SURFACE_BYTES, MAX_SURFACE_NODES, Surface, SurfaceRole, collection};
#[cfg(test)]
#[path = "hud_index_tests.rs"]
mod tests;

/// The index of an admitted HUD surface document, or none when control patches to it are
/// validated whole. Cloning shares it.
#[derive(Clone, Debug, Default)]
pub struct HudIndex(Option<Arc<Index>>);

#[derive(Debug)]
struct Index {
    /// The indexed surface's screen and document, which a patched surface must still carry.
    screen: String,
    document: String,
    namespace: String,
    /// The root definition's local name.
    root: String,
    definitions: BTreeMap<String, Arc<Indexed>>,
    /// The definitions naming each local name as an inheritance edge.
    readers: Arc<HashMap<String, Vec<String>>>,
    /// Authored nodes across every definition.
    authored_nodes: usize,
    /// Twice the largest literal parameter of any definition.
    parameter_bytes: usize,
    /// Known expanded property bytes under `parameter_bytes`, by local name.
    expanded: BTreeMap<String, usize>,
    /// Nodes of the resolved root, below the node budget.
    resolved_nodes: usize,
    /// The root's authored children, in order.
    parts: Vec<Part>,
}

/// One top-level definition with what validation reads from it.
#[derive(Debug)]
struct Indexed {
    /// The document key: the local name with an optional `@` base.
    key: String,
    base: Option<String>,
    body: Value,
    /// The body exactly as the patched document serializes it.
    text: String,
    authored_nodes: usize,
    /// Inheritance edges, the base first, then the bases of nested children.
    references: Vec<String>,
    largest_parameter: usize,
}

/// A root child and, once known, the resolved nodes below the root it contributes.
#[derive(Clone, Debug)]
struct Part {
    entry: Arc<Entry>,
    nodes: Option<usize>,
}

/// An authored root child entry, `{"name@base": body}`.
#[derive(Debug)]
struct Entry {
    value: Value,
    text: String,
    /// The definitions the entry reads directly: its base and the bases of nested children.
    references: Vec<String>,
}

impl HudIndex {
    /// Validates `surface` as a HUD surface, exactly as [`Surface::validate_hud`] does, and
    /// indexes its document when control patches to it can be checked incrementally.
    pub fn new(surface: &Surface) -> Result<Self, String> {
        let (root, document) = surface.resolved()?;
        surface.validate_root(&root, SurfaceRole::Hud)?;
        Ok(Self(Index::build(surface, &root, document).map(Arc::new)))
    }

    /// Whether this index describes `surface`'s document.
    pub fn describes(&self, surface: &Surface) -> bool {
        self.index(surface).is_some()
    }

    /// The index when it describes `surface`.
    fn index(&self, surface: &Surface) -> Option<&Index> {
        self.0.as_deref().filter(|index| {
            index.screen == surface.screen
                && index.document == surface.document
                && surface.collections.is_empty()
        })
    }

    /// The document `controls` patches `current`'s into and its index, when both are proven
    /// admissible without validating the whole document; none otherwise, for any reason.
    pub(super) fn patched(
        &self,
        current: &Surface,
        controls: &BTreeMap<String, Option<Value>>,
    ) -> Option<(String, Self)> {
        let next = self.index(current)?.patch(controls)?;
        Some((next.document.clone(), Self(Some(Arc::new(next)))))
    }
}

impl Index {
    /// Indexes a document whole-document validation admitted, given its resolved `root` and
    /// parsed members; none when it has collections, a root base, an `@` in a string value or a
    /// resolved tree at the node budget.
    fn build(surface: &Surface, root: &Value, document: Map<String, Value>) -> Option<Self> {
        let (namespace, root_name) = surface.screen.split_once('.')?;
        if !surface.collections.is_empty() {
            return None;
        }
        let resolved_nodes = collection::count(root);
        if resolved_nodes >= MAX_SURFACE_NODES {
            return None;
        }
        let mut definitions = BTreeMap::new();
        for (key, body) in document {
            if key == "namespace" {
                continue;
            }
            if marked(&body) {
                return None;
            }
            let (local, indexed) = Indexed::new(key, body, namespace).ok()?;
            definitions.insert(local, Arc::new(indexed));
        }
        let root_definition = definitions.get(root_name)?;
        if root_definition.base.is_some() {
            return None;
        }
        let entries = children(&root_definition.body)?;
        // Below the node budget, every authored root child that is not `ignored` resolves to
        // one resolved root child, in order: with as many of each, they correspond one to one.
        let resolved: Vec<&Value> = root
            .get("controls")
            .and_then(Value::as_array)
            .map(|children| children.iter().collect())
            .unwrap_or_default();
        let aligned = resolved.len() == entries.len();
        let parts = entries
            .iter()
            .zip(0..)
            .map(|(entry, position)| {
                let nodes = aligned
                    .then(|| resolved[position].as_object())
                    .flatten()
                    .and_then(|named| named.values().next())
                    .map(collection::count);
                Some(Part {
                    entry: Arc::new(Entry::new(entry, namespace)?),
                    nodes,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let parameter_bytes = parameter_bytes(&definitions);
        let mut expanded = BTreeMap::new();
        definition_bytes(
            root_name,
            namespace,
            &lookup(&definitions),
            parameter_bytes,
            &mut expanded,
        )
        .ok()?;
        Some(Self {
            screen: surface.screen.clone(),
            document: surface.document.clone(),
            namespace: namespace.to_owned(),
            root: root_name.to_owned(),
            authored_nodes: definitions.values().map(|d| d.authored_nodes).sum(),
            readers: Arc::new(readers(&definitions)),
            definitions,
            parameter_bytes,
            expanded,
            resolved_nodes,
            parts,
        })
    }

    /// This document with `controls` applied, when every check whole-document validation would
    /// make is proven to pass from the changed definitions and the root children reading them.
    fn patch(&self, controls: &BTreeMap<String, Option<Value>>) -> Option<Self> {
        let namespace = self.namespace.as_str();
        let mut definitions = self.definitions.clone();
        let mut changed = BTreeSet::new();
        let mut authored_nodes = self.authored_nodes;
        for edit in edits(namespace, |local| definitions.contains_key(local), controls).ok()? {
            if let Some(previous) = definitions.remove(&edit.local) {
                authored_nodes -= previous.authored_nodes;
            }
            if let Some(body) = edit.body {
                if marked(body) {
                    return None;
                }
                let (local, indexed) =
                    Indexed::new(edit.key.to_owned(), body.clone(), namespace).ok()?;
                authored_nodes += indexed.authored_nodes;
                definitions.insert(local, Arc::new(indexed));
            }
            changed.insert(edit.local);
        }
        if authored_nodes > MAX_SURFACE_NODES {
            return None;
        }
        let root = definitions.get(&self.root)?;
        let old_root = self.definitions.get(&self.root)?;
        if changed.contains(&self.root)
            && (root.base.is_some() || !same_properties(&root.body, &old_root.body))
        {
            return None;
        }
        // The inheritance graph changes only where a changed definition's edges do; otherwise
        // its checks all pass as they did.
        let regraphed = changed.iter().any(|name| {
            definitions.get(name).map(|d| &d.references)
                != self.definitions.get(name).map(|d| &d.references)
        });
        let readers = if regraphed {
            Arc::new(readers(&definitions))
        } else {
            Arc::clone(&self.readers)
        };
        // Every definition reading a changed one; only these can reach a changed edge.
        let reach = reach(&readers, &changed);
        if regraphed {
            let graph = |key: &str| definitions.get(key).map(|d| d.references.as_slice());
            for key in reach.iter().filter(|key| definitions.contains_key(**key)) {
                acyclic(key, &graph, &mut Vec::new(), &mut BTreeSet::new()).ok()?;
            }
        }
        let parameter_bytes = parameter_bytes(&definitions);
        let mut expanded = if parameter_bytes == self.parameter_bytes {
            self.expanded
                .iter()
                .filter(|(name, _)| !reach.contains(name.as_str()))
                .map(|(name, bytes)| (name.clone(), *bytes))
                .collect()
        } else {
            BTreeMap::new()
        };
        definition_bytes(
            &self.root,
            namespace,
            &lookup(&definitions),
            parameter_bytes,
            &mut expanded,
        )
        .ok()?;
        // Whole-document validation also loads every definition into one catalog. That load
        // reports nothing for the canonical text of definitions `authored` admits: the reader
        // accepts all serialized JSON, and every load diagnostic is a shape `authored` rejects
        // or a repeated local name, which the definitions map rules out.
        let entries = if changed.contains(&self.root) {
            let old: Vec<&Arc<Entry>> = self.parts.iter().map(|part| &part.entry).collect();
            children(&root.body)?
                .iter()
                .zip(0..)
                .map(|(entry, position)| match old.get(position) {
                    Some(old) if old.value == *entry => Some(Arc::clone(old)),
                    _ => Entry::new(entry, namespace).map(Arc::new),
                })
                .collect::<Option<Vec<_>>>()?
        } else {
            self.parts.iter().map(|part| part.entry.clone()).collect()
        };
        let affected = |entry: &Entry| {
            entry
                .references
                .iter()
                .any(|name| reach.contains(name.as_str()))
        };
        let mut unchanged: HashMap<&str, Vec<usize>> = HashMap::new();
        for (position, part) in self.parts.iter().enumerate().rev() {
            if !affected(&part.entry) {
                unchanged
                    .entry(part.entry.text.as_str())
                    .or_default()
                    .push(position);
            }
        }
        let mut kept = vec![false; self.parts.len()];
        let mut parts = Vec::with_capacity(entries.len());
        let mut added_nodes = 0usize;
        for entry in entries {
            let same = (!affected(&entry))
                .then(|| unchanged.get_mut(entry.text.as_str()).and_then(Vec::pop))
                .flatten();
            let nodes = match same {
                Some(position) => {
                    kept[position] = true;
                    self.parts[position].nodes
                }
                None => {
                    let nodes = part_nodes(&self.screen, &root.body, &entry, &definitions)?;
                    added_nodes += nodes;
                    Some(nodes)
                }
            };
            parts.push(Part { entry, nodes });
        }
        let mut removed_nodes = 0usize;
        for (part, _) in self.parts.iter().zip(&kept).filter(|(_, kept)| !**kept) {
            removed_nodes += match part.nodes {
                Some(nodes) => nodes,
                None => part_nodes(&self.screen, &old_root.body, &part.entry, &self.definitions)?,
            };
        }
        let resolved_nodes = self
            .resolved_nodes
            .checked_sub(removed_nodes)?
            .checked_add(added_nodes)?;
        if resolved_nodes >= MAX_SURFACE_NODES {
            return None;
        }
        let document = document(
            namespace,
            definitions
                .values()
                .map(|definition| (definition.key.as_str(), definition.text.as_str()))
                .collect(),
        )?;
        if document.len() > MAX_SURFACE_BYTES {
            return None;
        }
        Some(Self {
            screen: self.screen.clone(),
            document,
            namespace: self.namespace.clone(),
            root: self.root.clone(),
            definitions,
            readers,
            authored_nodes,
            parameter_bytes,
            expanded,
            resolved_nodes,
            parts,
        })
    }
}

impl Indexed {
    /// Checks one authored definition as whole-document validation does and records its
    /// inheritance edges, returning it under its local name.
    fn new(key: String, body: Value, namespace: &str) -> Result<(String, Self), String> {
        let (local, base) = local_reference(&key, namespace)?;
        let mut references: Vec<String> = base.iter().cloned().collect();
        let mut authored_nodes = 0;
        authored(&body, namespace, 0, &mut authored_nodes, &mut references)?;
        Ok((
            local,
            Self {
                text: body.to_string(),
                largest_parameter: largest_parameter(&body),
                key,
                base,
                body,
                authored_nodes,
                references,
            },
        ))
    }
}

impl Entry {
    /// Reads one `{"name@base": body}` root child entry.
    fn new(value: &Value, namespace: &str) -> Option<Self> {
        let (key, body) = value.as_object()?.iter().next()?;
        let mut references: Vec<String> = local_reference(key, namespace)
            .ok()?
            .1
            .into_iter()
            .collect();
        authored(body, namespace, 1, &mut 0, &mut references).ok()?;
        Some(Self {
            text: value.to_string(),
            value: value.clone(),
            references,
        })
    }
}

/// Resolved nodes below the root that the root child `entry` contributes, resolving the root's
/// own properties and that child against only the definitions it reads; none unless the
/// child passes every HUD check alone.
fn part_nodes(
    screen: &str,
    root: &Value,
    entry: &Entry,
    definitions: &BTreeMap<String, Arc<Indexed>>,
) -> Option<usize> {
    let (namespace, root_name) = screen.split_once('.')?;
    let mut reads = BTreeSet::new();
    let mut pending: Vec<&str> = entry.references.iter().map(String::as_str).collect();
    while let Some(name) = pending.pop() {
        if reads.insert(name) {
            pending.extend(definitions.get(name)?.references.iter().map(String::as_str));
        }
    }
    if reads.contains(root_name) {
        return None;
    }
    let mut body: Map<String, Value> = root
        .as_object()?
        .iter()
        .filter(|(key, _)| *key != "controls")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    body.insert("controls".into(), Value::Array(vec![entry.value.clone()]));
    let root_text = Value::Object(body).to_string();
    let mut members = vec![(root_name, root_text.as_str())];
    for name in reads {
        let definition = definitions.get(name)?;
        members.push((definition.key.as_str(), definition.text.as_str()));
    }
    let part = Surface {
        screen: screen.to_owned(),
        document: document(namespace, members)?,
        bindings: BTreeMap::new(),
        transitions: BTreeMap::new(),
        collections: BTreeMap::new(),
    };
    // The part's definitions are a subset of the patched document's, which passed every static
    // check, so only resolution and the emitted tree's checks remain.
    let resolved = part.resolve_static().ok()?;
    part.validate_root(&resolved, SurfaceRole::Hud).ok()?;
    collection::count(&resolved).checked_sub(1)
}

/// The document text of `namespace` and `members`, each a document key and its serialized
/// body, in key order as a parsed document serializes.
fn document(namespace: &str, members: Vec<(&str, &str)>) -> Option<String> {
    let namespace = Value::String(namespace.to_owned()).to_string();
    let mut members = members;
    members.push(("namespace", namespace.as_str()));
    members.sort_unstable_by(|left, right| left.0.cmp(right.0));
    let mut text = Vec::with_capacity(
        members
            .iter()
            .map(|(key, body)| key.len() + body.len() + 4)
            .sum::<usize>()
            + 2,
    );
    text.push(b'{');
    for (position, (key, body)) in members.iter().enumerate() {
        if position > 0 {
            text.push(b',');
        }
        serde_json::to_writer(&mut text, key).ok()?;
        text.push(b':');
        text.extend_from_slice(body.as_bytes());
    }
    text.push(b'}');
    String::from_utf8(text).ok()
}

/// Looks up a definition's body and base for the expanded byte bound.
fn lookup<'a>(
    definitions: &'a BTreeMap<String, Arc<Indexed>>,
) -> impl Fn(&str) -> Option<(&'a Value, Option<&'a str>)> + 'a {
    |name| {
        definitions
            .get(name)
            .map(|definition| (&definition.body, definition.base.as_deref()))
    }
}

/// Twice the largest literal parameter of any definition, the bound on a substituted operand.
fn parameter_bytes(definitions: &BTreeMap<String, Arc<Indexed>>) -> usize {
    definitions
        .values()
        .map(|definition| definition.largest_parameter)
        .max()
        .unwrap_or(0)
        .saturating_mul(2)
}

/// The definitions naming each local name as an inheritance edge, missing names included.
fn readers(definitions: &BTreeMap<String, Arc<Indexed>>) -> HashMap<String, Vec<String>> {
    let mut readers: HashMap<String, Vec<String>> = HashMap::new();
    for (name, definition) in definitions {
        for read in &definition.references {
            readers.entry(read.clone()).or_default().push(name.clone());
        }
    }
    readers
}

/// `changed` and every definition reading one of them through inheritance edges,
/// transitively, including readers of removed definitions.
fn reach<'a>(
    readers: &'a HashMap<String, Vec<String>>,
    changed: &'a BTreeSet<String>,
) -> BTreeSet<&'a str> {
    let mut reach = BTreeSet::new();
    let mut pending: Vec<&str> = changed.iter().map(String::as_str).collect();
    while let Some(name) = pending.pop() {
        if reach.insert(name) {
            pending.extend(readers.get(name).into_iter().flatten().map(String::as_str));
        }
    }
    reach
}

/// The authored child entries of a control body, none for a malformed child list.
fn children(body: &Value) -> Option<&[Value]> {
    match body.get("controls") {
        None => Some(&[]),
        Some(children) => children.as_array().map(Vec::as_slice),
    }
}

/// Whether two control bodies have the same properties apart from their children.
fn same_properties(left: &Value, right: &Value) -> bool {
    /// The members of `body` apart from `controls`.
    fn properties(body: &Value) -> Option<Vec<(&String, &Value)>> {
        body.as_object().map(|members| {
            members
                .iter()
                .filter(|(key, _)| *key != "controls")
                .collect()
        })
    }
    properties(left) == properties(right)
}

/// Whether a string value under `value` contains `@`, which substitution could turn into an
/// animation reference.
fn marked(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains('@'),
        Value::Array(items) => items.iter().any(marked),
        Value::Object(members) => members.values().any(marked),
        _ => false,
    }
}
