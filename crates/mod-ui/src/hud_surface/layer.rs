//! One mod's HUD surface: its parts, their data, projections, transitions and embeddable
//! named controls.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use json_ui::{DataSource, Scalar};
use serde_json::{Value, json};
use ui::mod_panel::{
    Surface, SurfaceValue, definitions::Definitions, hud_control::control_name,
    projected_visible_name,
};

use super::document::{Document, PartSpec};
use super::part::Part;
use super::values::{self, ActorAuthority, ProjectedActor, RootPlacement};
use crate::{PanelUiOwner, mod_panel::transitions::SurfaceTransitions};

/// Host bindings with the root's size in GUI units.
pub(crate) const WIDTH: &str = "#surface_width";
pub(crate) const HEIGHT: &str = "#surface_height";
/// Named controls one mod keeps built for embedding; the least recently drawn one makes way
/// for another.
pub(crate) const MAX_EMBEDDED: usize = 128;

pub(crate) struct Layer {
    pub owner: PanelUiOwner,
    pub revision: u64,
    pub surface: Arc<Surface>,
    pub authority: ActorAuthority,
    /// The parts in paint order; empty when the document cannot be drawn.
    pub parts: Vec<Part>,
    /// Embeddable named controls, built on first use.
    pub embedded: BTreeMap<String, Embedded>,
    /// Counts the panel frames that drew embedded controls.
    embed_frame: u64,
    /// The document's authored definitions, which locate what a new document changes; none
    /// when it cannot be read.
    definitions: Option<Definitions>,
    /// Each floating part's projected offset, or none while it is hidden.
    pub moves: Vec<Option<[f64; 2]>>,
    /// Each part's gameplay replay placement.
    pub replays: Vec<ui_engine::engine::CachedFragment>,
    /// The last projected offset of each projected binding.
    projected: BTreeMap<String, Option<[f64; 2]>>,
    transitions: SurfaceTransitions,
    root: [f64; 2],
}

/// One embeddable named control and the last panel frame that drew it.
pub(crate) struct Embedded {
    /// None for a control that cannot draw.
    pub part: Option<Part>,
    drawn: u64,
}

/// How a new document of the same screen and collections changes the root's parts.
enum RootChange {
    Unchanged,
    /// The parts drawing these root children.
    Children(Vec<usize>),
    All,
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
        let parts = parts(&surface, root);
        let moves = vec![None; parts.len()];
        let replays = parts.iter().map(|_| Default::default()).collect();
        Self {
            owner,
            revision,
            definitions: Definitions::parse(&surface.document).ok(),
            surface,
            authority,
            parts,
            embedded: BTreeMap::new(),
            embed_frame: 0,
            moves,
            replays,
            projected: BTreeMap::new(),
            transitions: SurfaceTransitions::default(),
            root,
        }
    }

    /// Takes a newer publication of the same screen and collections, rebuilding only the
    /// parts and embedded controls whose resolution reads a changed definition, or that
    /// floated on a binding no longer projected, and updating only the parts that read a
    /// changed binding or collection. A removed binding reads as in a layer built for the new
    /// publication. Returns false when the layer must be built anew.
    pub fn publish(
        &mut self,
        revision: u64,
        surface: Arc<Surface>,
        authority: ActorAuthority,
    ) -> bool {
        let redefined = if self.surface.same_definition(&surface) {
            None
        } else {
            match self.redefinition(&surface) {
                Some(redefined) => Some(redefined),
                None => return false,
            }
        };
        self.revision = revision;
        let previous = std::mem::replace(&mut self.surface, surface);
        self.authority = authority;
        if let Some((definitions, affected, change)) = redefined {
            self.redefine(definitions, &affected, change);
        }
        let unprojected: Vec<String> = previous
            .bindings
            .iter()
            .filter(|(name, value)| value.projection().is_some() && !projected(&self.surface, name))
            .map(|(name, _)| name.clone())
            .collect();
        for name in &unprojected {
            self.projected.remove(name);
        }
        if !self.settle_floating() {
            return false;
        }
        let mut names = changed_bindings(&previous, &self.surface);
        names.extend(
            previous
                .transitions
                .keys()
                .filter(|name| !self.surface.transitions.contains_key(*name))
                .cloned(),
        );
        names.extend(
            unprojected
                .iter()
                .map(|name| projected_visible_name(name))
                .filter(|companion| !self.surface.bindings.contains_key(companion)),
        );
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

    /// Rebuilds each part floating on a binding that is no longer projected, as a part built
    /// for the current surface would not float on it. False, for the layer to be built anew,
    /// when one cannot be rebuilt alone.
    fn settle_floating(&mut self) -> bool {
        let mut stranded = Vec::new();
        for part in &self.parts {
            if part
                .spec
                .floating
                .as_ref()
                .is_some_and(|floating| !projected(&self.surface, &floating.offset))
            {
                match part.spec.child {
                    Some(child) => stranded.push(child),
                    None => return false,
                }
            }
        }
        if stranded.is_empty() {
            return true;
        }
        let Some(definitions) = self.definitions.take() else {
            return false;
        };
        let rebuilt = self.rebuild_children(&definitions, &stranded);
        self.definitions = Some(definitions);
        rebuilt
    }

    /// Every part, embedded ones included.
    fn parts_mut(&mut self) -> impl Iterator<Item = &mut Part> {
        self.parts.iter_mut().chain(
            self.embedded
                .values_mut()
                .filter_map(|embedded| embedded.part.as_mut()),
        )
    }

    /// The new definitions, those whose resolution reads a changed one, and how the root's
    /// parts change, for a document of the same screen and collections; none when the layer
    /// must be built anew.
    fn redefinition(&self, next: &Surface) -> Option<(Definitions, BTreeSet<String>, RootChange)> {
        if !self.surface.same_screen_and_collections(next) {
            return None;
        }
        let old = self.definitions.as_ref()?;
        let new = Definitions::parse(&next.document).ok()?;
        let affected = new.affected(&old.changed(&new));
        let name = next.screen.split_once('.')?.1;
        let change = match (old.definitions.get(name), new.definitions.get(name)) {
            _ if self.parts.is_empty() => RootChange::All,
            _ if !affected.contains(name) => RootChange::Unchanged,
            (Some(old_root), Some(new_root))
                if old_root.base.is_none()
                    && new_root.base.is_none()
                    && self.parts.iter().all(|part| part.spec.child.is_some()) =>
            {
                changed_children(&new, &old_root.body, &new_root.body, &affected)
                    .map_or(RootChange::All, RootChange::Children)
            }
            _ => RootChange::All,
        };
        Some((new, affected, change))
    }

    /// Adopts the current surface's `definitions`: drops embedded controls whose resolution
    /// reads a changed definition and rebuilds the affected parts, every part when the
    /// changed children cannot be rebuilt alone.
    fn redefine(
        &mut self,
        definitions: Definitions,
        affected: &BTreeSet<String>,
        change: RootChange,
    ) {
        self.embedded
            .retain(|control, _| !affected.contains(control));
        let rebuilt = match change {
            RootChange::Unchanged => true,
            RootChange::Children(children) => self.rebuild_children(&definitions, &children),
            RootChange::All => false,
        };
        if !rebuilt {
            self.parts = parts(&self.surface, self.root);
            self.moves = vec![None; self.parts.len()];
            self.replays = self.parts.iter().map(|_| Default::default()).collect();
            self.projected.clear();
        }
        self.definitions = Some(definitions);
    }

    /// Rebuilds the parts drawing root `children`, each resolved alone; false, changing
    /// nothing, when one cannot be built or would change the paint order.
    fn rebuild_children(&mut self, definitions: &Definitions, children: &[usize]) -> bool {
        let mut built = Vec::new();
        for child in children {
            let Some(index) = self
                .parts
                .iter()
                .position(|part| part.spec.child == Some(*child))
            else {
                return false;
            };
            match Document::child(&self.surface, definitions, *child) {
                Ok((catalog, spec)) if spec.layer == self.parts[index].spec.layer => {
                    built.push((index, catalog, spec));
                }
                Ok(_) => return false,
                Err(error) => {
                    tracing::debug!(%error, "HUD surface control rebuilds with its root");
                    return false;
                }
            }
        }
        let displayed: Vec<(String, Scalar)> = self.transitions.displayed().collect();
        for (index, catalog, spec) in built {
            let mut part = part(&catalog, spec, &self.surface, self.root);
            let floating = part
                .spec
                .floating
                .as_ref()
                .map(|floating| floating.offset.clone());
            for (name, offset) in &self.projected {
                if floating.as_ref() != Some(name) {
                    write_projection(&mut part, name, &projected_visible_name(name), *offset);
                }
            }
            for (name, value) in &displayed {
                if part.spec.bindings.contains(name) {
                    part.set_global(name, value.clone());
                }
            }
            self.moves[index] = part
                .spec
                .floating
                .as_ref()
                .and_then(|floating| self.projected.get(&floating.offset).copied().flatten());
            self.replays[index] = Default::default();
            self.parts[index] = part;
        }
        true
    }

    /// Writes binding `name`'s current value into each part that reads it; an unbound name
    /// reads as in a part built for the current surface.
    fn apply(&mut self, name: &str) {
        let value = self.surface.bindings.get(name).cloned();
        if value
            .as_ref()
            .is_some_and(|value| value.projection().is_some())
        {
            self.projected.remove(name);
            let visible = projected_visible_name(name);
            for (index, part) in self.parts.iter_mut().enumerate() {
                if part
                    .spec
                    .floating
                    .as_ref()
                    .is_some_and(|floating| floating.offset == name)
                {
                    self.moves[index] = None;
                } else {
                    part.set_item(name, None);
                    write_projection(part, name, &visible, None);
                }
            }
            for part in self
                .embedded
                .values_mut()
                .filter_map(|embedded| embedded.part.as_mut())
            {
                part.set_item(name, None);
                write_projection(part, name, &visible, None);
            }
            return;
        }
        let transitioning = self.surface.transitions.contains_key(name)
            && value.as_ref().and_then(SurfaceValue::numbers).is_some();
        let (surface, root) = (&self.surface, self.root);
        let embedded = self
            .embedded
            .values_mut()
            .filter_map(|embedded| embedded.part.as_mut());
        for part in self.parts.iter_mut().chain(embedded) {
            if !part.spec.bindings.contains(name)
                || part
                    .spec
                    .floating
                    .as_ref()
                    .is_some_and(|floating| floating.offset == name)
            {
                continue;
            }
            if !matches!(value, Some(SurfaceValue::Item(_))) {
                part.set_item(name, None);
            }
            if transitioning {
                continue;
            }
            match &value {
                Some(SurfaceValue::Item(item)) => part.set_item(name, Some(item.item.clone())),
                Some(SurfaceValue::Project(_)) => {}
                Some(value) => part.set_global(name, values::scalar(value)),
                None => match initial(&part.spec, surface, root, name) {
                    Some(value) => part.set_global(name, value),
                    None => part.unset_global(name),
                },
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
        if self.transitions.advance(&self.surface, now, animate) {
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
            if value.projection().is_none() {
                continue;
            }
            let offset = value.active_projection().and_then(|projection| {
                values::project(projection, camera, root, actor, self.authority)
            });
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
            for part in self
                .embedded
                .values_mut()
                .filter_map(|embedded| embedded.part.as_mut())
            {
                write_projection(part, name, &visible, offset);
            }
        }
    }

    /// Starts a panel frame of embedded control draws, which [`Self::embedded`] stamps.
    pub fn begin_embeds(&mut self) {
        self.embed_frame += 1;
    }

    /// The embeddable named control `control`, built on first use. At [`MAX_EMBEDDED`] built
    /// controls, the one least recently drawn makes way, unless every one was drawn this
    /// frame.
    pub fn embedded(&mut self, control: &str) -> Option<&mut Part> {
        if !self.embedded.contains_key(control) {
            if !control_name(control) {
                return None;
            }
            if self.embedded.len() >= MAX_EMBEDDED {
                let evicted = self
                    .embedded
                    .iter()
                    .filter(|(_, embedded)| embedded.drawn < self.embed_frame)
                    .min_by_key(|(_, embedded)| embedded.drawn)
                    .map(|(name, _)| name.clone())?;
                self.embedded.remove(&evicted);
            }
            let built = match Document::embedded(&self.surface, self.definitions.as_ref(), control)
            {
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
            self.embedded.insert(
                control.to_owned(),
                Embedded {
                    part: built,
                    drawn: 0,
                },
            );
        }
        let embedded = self.embedded.get_mut(control)?;
        embedded.drawn = self.embed_frame;
        embedded.part.as_mut()
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

/// Every part of `surface`'s root with its initial data; none when the document cannot be
/// drawn.
fn parts(surface: &Surface, root: [f64; 2]) -> Vec<Part> {
    match Document::build(surface) {
        Ok(document) => document
            .parts
            .into_iter()
            .map(|spec| part(&document.catalog, spec, surface, root))
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "HUD surface cannot be drawn");
            Vec::new()
        }
    }
}

/// The root children whose resolution changes between two root bodies with the same
/// properties and number of children: changed entries and entries reading an `affected`
/// definition. None when anything else about the root changed.
fn changed_children(
    definitions: &Definitions,
    old: &Value,
    new: &Value,
    affected: &BTreeSet<String>,
) -> Option<Vec<usize>> {
    let (Value::Object(old), Value::Object(new)) = (old, new) else {
        return None;
    };
    let properties = |object: &serde_json::Map<String, Value>| {
        object
            .iter()
            .filter(|(key, _)| *key != "controls")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<serde_json::Map<_, _>>()
    };
    if properties(old) != properties(new) {
        return None;
    }
    let (old, new) = (children(old)?, children(new)?);
    if old.len() != new.len() {
        return None;
    }
    Some(
        old.iter()
            .zip(new)
            .enumerate()
            .filter(|(_, (old, new))| {
                old != new
                    || definitions
                        .references(&json!({"controls":[new]}))
                        .iter()
                        .any(|name| affected.contains(name))
            })
            .map(|(index, _)| index)
            .collect(),
    )
}

/// An authored control's named children; none when they are not a list.
fn children(control: &serde_json::Map<String, Value>) -> Option<&[Value]> {
    control.get("controls").map_or(Some(&[][..]), |children| {
        children.as_array().map(Vec::as_slice)
    })
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
        if let Some(value) = initial(&spec, surface, root, name) {
            data.set_global(name, value);
        }
        if let Some(SurfaceValue::Item(item)) = surface.bindings.get(name) {
            items.push((name.clone(), item.item.clone()));
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

/// The value a part `spec` built for `surface` starts binding `name` at: a floating part's
/// own offset and companion, a bound value, with items unresolved, the root size, or a
/// projection's hidden companion. None leaves `name` unbound.
fn initial(spec: &PartSpec, surface: &Surface, root: [f64; 2], name: &str) -> Option<Scalar> {
    let floating = spec.floating.as_ref();
    if floating.is_some_and(|floating| floating.offset == name) {
        Some(Scalar::Json(json!([root[0] * 0.5, root[1] * 0.5])))
    } else if floating.is_some_and(|floating| floating.visible.as_deref() == Some(name)) {
        Some(Scalar::Bool(true))
    } else if let Some(value) = surface.bindings.get(name) {
        Some(values::scalar(value))
    } else if name == WIDTH {
        Some(Scalar::Num(root[0]))
    } else if name == HEIGHT {
        Some(Scalar::Num(root[1]))
    } else if name
        .strip_suffix(ui::mod_panel::PROJECTED_VISIBLE_SUFFIX)
        .is_some_and(|base| projected(surface, base))
    {
        Some(Scalar::Bool(false))
    } else {
        None
    }
}

/// Whether `surface` binds `name` to a projected value.
fn projected(surface: &Surface, name: &str) -> bool {
    surface
        .bindings
        .get(name)
        .is_some_and(|value| value.projection().is_some())
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
            crate::mod_panel::surface::collection_item(&collection.definition.items[*index])
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
