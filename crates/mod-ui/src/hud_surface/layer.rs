//! One mod's HUD surface: its parts, their data, projections, transitions and embeddable
//! named controls.

use std::collections::BTreeMap;
use std::sync::Arc;

use json_ui::{DataSource, Scalar};
use serde_json::json;
use ui::mod_panel::{Surface, SurfaceValue, hud_control::control_name, projected_visible_name};

use super::document::{Document, PartSpec};
use super::part::Part;
use super::values::{self, ActorAuthority, ProjectedActor, RootPlacement};
use crate::{PanelUiOwner, mod_panel::transitions::SurfaceTransitions};

/// Host bindings with the root's size in GUI units.
pub(crate) const WIDTH: &str = "#surface_width";
pub(crate) const HEIGHT: &str = "#surface_height";
/// Named controls one mod may embed at once.
pub(crate) const MAX_EMBEDDED: usize = 64;

pub(crate) struct Layer {
    pub owner: PanelUiOwner,
    pub revision: u64,
    pub surface: Arc<Surface>,
    pub authority: ActorAuthority,
    /// The parts in paint order; empty when the document cannot be drawn.
    pub parts: Vec<Part>,
    /// Embeddable named controls, built on first use; none for a control that cannot draw.
    pub embedded: BTreeMap<String, Option<Part>>,
    /// Each floating part's projected offset, or none while it is hidden.
    pub moves: Vec<Option<[f64; 2]>>,
    /// Each part's gameplay replay placement.
    pub replays: Vec<ui_engine::engine::CachedFragment>,
    /// The last projected offset of each projected binding.
    projected: BTreeMap<String, Option<[f64; 2]>>,
    transitions: SurfaceTransitions,
    root: [f64; 2],
}

impl Layer {
    /// Builds a layer for a newly published surface; an invalid document draws nothing.
    pub fn new(
        owner: PanelUiOwner,
        revision: u64,
        surface: Arc<Surface>,
        authority: ActorAuthority,
        root: [f64; 2],
    ) -> Self {
        let parts = match Document::build(&surface) {
            Ok(document) => document
                .parts
                .into_iter()
                .map(|spec| part(&document.catalog, spec, &surface, root))
                .collect(),
            Err(error) => {
                tracing::warn!(%error, "HUD surface cannot be drawn");
                Vec::new()
            }
        };
        let moves = vec![None; parts.len()];
        let replays = parts.iter().map(|_| Default::default()).collect();
        Self {
            owner,
            revision,
            surface,
            authority,
            parts,
            embedded: BTreeMap::new(),
            moves,
            replays,
            projected: BTreeMap::new(),
            transitions: SurfaceTransitions::default(),
            root,
        }
    }

    /// Takes a newer publication of the same document, updating only the parts that read a
    /// changed binding or collection. Returns false when the document changed.
    pub fn publish(
        &mut self,
        revision: u64,
        surface: Arc<Surface>,
        authority: ActorAuthority,
    ) -> bool {
        if !self.surface.same_definition(&surface) {
            return false;
        }
        self.revision = revision;
        let previous = std::mem::replace(&mut self.surface, surface);
        self.authority = authority;
        let names = changed_bindings(&previous, &self.surface);
        for name in names {
            self.apply(&name);
        }
        let surface = Arc::clone(&self.surface);
        for (name, collection) in &surface.collections {
            if previous.collections.get(name).map(|old| &old.order) != Some(&collection.order) {
                for part in self.parts_mut() {
                    if part.spec.collections.contains(name) {
                        write_collection(part, &surface, name);
                    }
                }
            }
        }
        true
    }

    /// Every part, embedded ones included.
    fn parts_mut(&mut self) -> impl Iterator<Item = &mut Part> {
        self.parts
            .iter_mut()
            .chain(self.embedded.values_mut().flatten())
    }

    /// Writes binding `name`'s current value into each part that reads it.
    fn apply(&mut self, name: &str) {
        let value = self.surface.bindings.get(name).cloned();
        if value
            .as_ref()
            .is_some_and(|value| value.projection().is_some())
        {
            self.projected.remove(name);
        }
        if self.surface.transitions.contains_key(name)
            && value.as_ref().and_then(SurfaceValue::numbers).is_some()
        {
            return;
        }
        for part in self.parts_mut() {
            if !part.spec.bindings.contains(name)
                || part
                    .spec
                    .floating
                    .as_ref()
                    .is_some_and(|floating| floating.offset == name)
            {
                continue;
            }
            match &value {
                Some(SurfaceValue::Item(item)) => part.set_item(name, Some(item.item.clone())),
                Some(SurfaceValue::Project(_)) => {}
                Some(value) => part.set_global(name, values::scalar(value)),
                None => {
                    part.set_item(name, None);
                    part.set_global(name, Scalar::Json(serde_json::Value::Null));
                }
            }
        }
    }

    /// The root size the layer's data was written for.
    pub fn root(&self) -> [f64; 2] {
        self.root
    }

    /// Advances transitions and follows a changed root size.
    pub fn advance(&mut self, now: f64, animate: bool, root: [f64; 2]) {
        if self.root != root {
            self.root = root;
            let center = json!([root[0] * 0.5, root[1] * 0.5]);
            for part in self.parts_mut() {
                part.set_global(WIDTH, Scalar::Num(root[0]));
                part.set_global(HEIGHT, Scalar::Num(root[1]));
                if let Some(floating) = part.spec.floating.clone() {
                    part.set_global(&floating.offset, Scalar::Json(center.clone()));
                }
            }
            self.projected.clear();
        }
        if !self.surface.transitions.is_empty()
            && self.transitions.advance(&self.surface, now, animate)
        {
            let displayed: Vec<(String, Scalar)> = self.transitions.displayed().collect();
            for part in self.parts_mut() {
                for (name, value) in &displayed {
                    if part.spec.bindings.contains(name) {
                        part.set_global(name, value.clone());
                    }
                }
            }
        }
    }

    /// Projects every projected binding, writing changed offsets and companions into the
    /// parts that read them and moving floating parts.
    pub fn project(
        &mut self,
        root: RootPlacement,
        camera: Option<&glam::Mat4>,
        actor: &dyn Fn(u64) -> Option<ProjectedActor>,
    ) {
        let surface = Arc::clone(&self.surface);
        for (name, value) in &surface.bindings {
            let Some(projection) = value.projection() else {
                continue;
            };
            let offset = values::project(projection, camera, root, actor, self.authority);
            if self.projected.get(name) == Some(&offset) {
                continue;
            }
            self.projected.insert(name.clone(), offset);
            let visible = projected_visible_name(name);
            for (index, part) in self.parts.iter_mut().enumerate() {
                if part
                    .spec
                    .floating
                    .as_ref()
                    .is_some_and(|floating| floating.offset == *name)
                {
                    self.moves[index] = offset;
                } else {
                    write_projection(part, name, &visible, offset);
                }
            }
            for part in self.embedded.values_mut().flatten() {
                write_projection(part, name, &visible, offset);
            }
        }
    }

    /// The embeddable named control `control`, built on first use.
    pub fn embedded(&mut self, control: &str) -> Option<&mut Part> {
        if !self.embedded.contains_key(control) {
            if self.embedded.len() >= MAX_EMBEDDED || !control_name(control) {
                return None;
            }
            let built = match Document::embedded(&self.surface, control) {
                Ok((catalog, spec)) => {
                    let mut part = part(&catalog, spec, &self.surface, self.root);
                    for (name, offset) in &self.projected {
                        write_projection(&mut part, name, &projected_visible_name(name), *offset);
                    }
                    Some(part)
                }
                Err(error) => {
                    tracing::warn!(control, %error, "embedded HUD control cannot be drawn");
                    None
                }
            };
            self.embedded.insert(control.to_owned(), built);
        }
        self.embedded.get_mut(control)?.as_mut()
    }

    /// The resolved value of every binding, as parts read it, for probes and tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn resolved(&self) -> BTreeMap<String, Scalar> {
        let mut values = BTreeMap::new();
        for part in &self.parts {
            for name in &part.spec.bindings {
                if let Some(value) = part.data().global_value(name) {
                    values.entry(name.clone()).or_insert_with(|| value.clone());
                }
            }
        }
        for (name, offset) in &self.projected {
            values.insert(name.clone(), Scalar::Json(json!(offset.unwrap_or([0.; 2]))));
            values.insert(projected_visible_name(name), Scalar::Bool(offset.is_some()));
        }
        values
    }
}

/// Names whose value differs between two surfaces, found in one merge of their sorted maps.
fn changed_bindings(previous: &Surface, next: &Surface) -> Vec<String> {
    let mut changed = Vec::new();
    let mut old = previous.bindings.iter().peekable();
    let mut new = next.bindings.iter().peekable();
    loop {
        match (old.peek(), new.peek()) {
            (None, None) => break,
            (Some((name, _)), None) => {
                changed.push((*name).clone());
                old.next();
            }
            (None, Some((name, _))) => {
                changed.push((*name).clone());
                new.next();
            }
            (Some((a, x)), Some((b, y))) => match a.cmp(b) {
                std::cmp::Ordering::Less => {
                    changed.push((*a).clone());
                    old.next();
                }
                std::cmp::Ordering::Greater => {
                    changed.push((*b).clone());
                    new.next();
                }
                std::cmp::Ordering::Equal => {
                    if x != y {
                        changed.push((*a).clone());
                    }
                    old.next();
                    new.next();
                }
            },
        }
    }
    changed
}

/// Writes a projected offset and its companion into a part that reads them.
fn write_projection(part: &mut Part, name: &str, visible: &str, offset: Option<[f64; 2]>) {
    if part.spec.bindings.contains(name) {
        part.set_global(name, Scalar::Json(json!(offset.unwrap_or([0.; 2]))));
    }
    if part.spec.bindings.contains(visible) {
        part.set_global(visible, Scalar::Bool(offset.is_some()));
    }
}

/// A part with its initial data: plain values, unresolved items, hidden projections, the
/// root size and its collections.
fn part(
    catalog: &Arc<json_ui::Catalog>,
    spec: PartSpec,
    surface: &Surface,
    root: [f64; 2],
) -> Part {
    let mut data = DataSource::new();
    let mut items = Vec::new();
    for name in &spec.bindings {
        let floating = spec.floating.as_ref();
        if floating.is_some_and(|floating| floating.offset == *name) {
            data.set_global(name, Scalar::Json(json!([root[0] * 0.5, root[1] * 0.5])));
        } else if floating.is_some_and(|floating| floating.visible.as_ref() == Some(name)) {
            data.set_global(name, Scalar::Bool(true));
        } else if let Some(value) = surface.bindings.get(name) {
            data.set_global(name, values::scalar(value));
            if let SurfaceValue::Item(item) = value {
                items.push((name.clone(), item.item.clone()));
            }
        } else if name == WIDTH {
            data.set_global(name, Scalar::Num(root[0]));
        } else if name == HEIGHT {
            data.set_global(name, Scalar::Num(root[1]));
        } else if name
            .strip_suffix(ui::mod_panel::PROJECTED_VISIBLE_SUFFIX)
            .and_then(|base| surface.bindings.get(base))
            .is_some_and(|value| value.projection().is_some())
        {
            data.set_global(name, Scalar::Bool(false));
        }
    }
    let collections = spec.collections.clone();
    let mut part = Part::new(Arc::clone(catalog), spec, data);
    for (name, item) in items {
        part.set_item(&name, Some(item));
    }
    for name in &collections {
        write_collection(&mut part, surface, name);
    }
    part
}

/// Publishes one collection's filtered items to a part's grid.
fn write_collection(part: &mut Part, surface: &Surface, name: &str) {
    let Some(collection) = surface.collections.get(name) else {
        return;
    };
    let items = collection
        .order
        .iter()
        .map(|index| {
            let mut entry = json_ui::CollectionItem::default();
            entry
                .values
                .insert("#item_exists".into(), Scalar::Bool(true));
            for (key, value) in &collection.definition.items[*index].values {
                entry.values.insert(key.clone(), values::scalar(value));
            }
            entry
        })
        .collect();
    let rows = collection
        .order
        .len()
        .div_ceil(collection.definition.columns);
    part.set_collection(
        name,
        items,
        Scalar::Json(json!([collection.definition.columns, rows])),
    );
}
