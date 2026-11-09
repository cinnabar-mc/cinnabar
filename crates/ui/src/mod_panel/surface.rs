//! Bounded extension-authored JSON-UI and its changing scalar bindings.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
pub mod backdrop;
mod collection;
pub mod definitions;
pub mod hud_control;
mod hud_index;
pub mod local_player_head;
mod paint_transform;
mod patch;
mod shadow;
mod static_templates;
mod transition;
mod value;
pub use collection::{CollectionArt, CollectionDefinition, CollectionItem, SurfaceCollection};
pub use hud_index::HudIndex;
pub use patch::{MAX_SURFACE_PATCH_BYTES, SurfacePatch};
use static_templates::{acyclic, authored, bound_expansion, local_reference, resolved_value};
pub use transition::{MAX_TRANSITION_SECONDS, SurfaceTransition, TransitionEasing};
use value::bounded;
pub use value::{
    ActorPoint, ItemValue, PROJECTED_VISIBLE_SUFFIX, ProjectValue, Projection, SurfaceItem,
    SurfaceValue, WorldPoint, projected_visible_name,
};

/// Draws the item an `{"item":..}` value bound to `#item_renderer_data` names.
pub const ITEM_RENDERER: &str = "inventory_item_renderer";

/// Host-owned track action consumed without invoking an extension control.
pub const SCROLL_TRACK_ACTION: &str = "mod.scroll_track";

/// Document bytes one surface may carry; first-party HUD and layout documents reach about
/// 96 KiB, so twice that leaves them headroom.
pub const MAX_SURFACE_BYTES: usize = 192 * 1024;
pub const MAX_SURFACE_NODES: usize = 4096;
/// Bindings and transitions one panel surface retains.
pub const MAX_SURFACE_BINDINGS: usize = 256;
/// Bindings and transitions one HUD surface retains; a full HUD has many live rows.
pub const MAX_HUD_SURFACE_BINDINGS: usize = 512;
pub(crate) const MAX_BINDING_NAME_BYTES: usize = 96;

/// Where a surface is presented, which selects its binding budget and permitted nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceRole {
    /// An input-owning personal or packet-mod panel.
    Panel,
    /// A mod's gameplay HUD layer: never hit-tested and never embedding HUD controls.
    Hud,
}

impl SurfaceRole {
    /// The most bindings, and separately transitions, a surface in this role retains.
    pub fn max_bindings(self) -> usize {
        match self {
            Self::Panel => MAX_SURFACE_BINDINGS,
            Self::Hud => MAX_HUD_SURFACE_BINDINGS,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surface {
    pub screen: String,
    pub document: String,
    #[serde(default)]
    pub bindings: BTreeMap<String, SurfaceValue>,
    /// Numeric bindings that ease toward each new value, keyed by binding name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub transitions: BTreeMap<String, SurfaceTransition>,
    /// Bounded immutable tile data and the current filtered order of each native grid.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub collections: BTreeMap<String, SurfaceCollection>,
}

impl Surface {
    /// Rejects oversized trees and dynamic factories before a panel retains them.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_as(SurfaceRole::Panel)
    }

    /// Rejects oversized trees, dynamic factories and input actions before a HUD retains them.
    pub fn validate_hud(&self) -> Result<(), String> {
        self.validate_as(SurfaceRole::Hud)
    }

    /// Validates the document for `role`, then its bindings within that role's budget.
    pub fn validate_as(&self, role: SurfaceRole) -> Result<(), String> {
        self.validate_root(&self.resolved_root()?, role)
    }

    /// Validates for `role` and returns the canonical static document, resolving the root
    /// once for both.
    pub fn validated_document(&self, role: SurfaceRole) -> Result<String, String> {
        let root = self.resolved_root()?;
        self.validate_root(&root, role)?;
        self.document_from(root)
    }

    /// [`Self::validate_as`] over an already resolved root.
    fn validate_root(&self, root: &Value, role: SurfaceRole) -> Result<(), String> {
        let actions = self.walk_root(root, role)?;
        if role == SurfaceRole::Hud && !actions.is_empty() {
            return Err("HUD surfaces cannot declare input actions".into());
        }
        self.validate_bindings_as(role)
    }

    /// Validates only changing panel values and their declared transitions, retaining the
    /// already checked document. A transition may name a binding a later update supplies.
    pub fn validate_bindings(&self) -> Result<(), String> {
        self.validate_bindings_as(SurfaceRole::Panel)
    }

    /// [`Self::validate_bindings`] within `role`'s binding budget. Projected values are global
    /// bindings only, and their host-published `<name>_visible` companions cannot be authored.
    pub fn validate_bindings_as(&self, role: SurfaceRole) -> Result<(), String> {
        if self.bindings.len() > role.max_bindings() {
            return Err("surface has too many bindings".into());
        }
        for (name, value) in &self.bindings {
            if !binding_name(name) {
                return Err("surface binding name is invalid".into());
            }
            value.validate()?;
            backdrop::validate_world_binding(name, value)?;
            if value.projection().is_some() {
                let companion = projected_visible_name(name);
                if !binding_name(&companion) || self.bindings.contains_key(&companion) {
                    return Err(format!(
                        "surface binding {companion} is reserved for {name}'s projection"
                    ));
                }
            }
        }
        if self.transitions.len() > role.max_bindings() {
            return Err("surface has too many transitions".into());
        }
        for (name, transition) in &self.transitions {
            if !binding_name(name) {
                return Err("surface transition name is invalid".into());
            }
            transition.validate()?;
        }
        for collection in self.collections.values() {
            collection.validate_order()?;
        }
        Ok(())
    }

    /// Returns authored pressed actions so the owning capability validates routes.
    pub fn actions(&self) -> Result<Vec<String>, String> {
        self.walk(SurfaceRole::Panel)
    }

    /// Returns a canonical static document consumed by the native renderer.
    /// No host context can later select an authored branch that escaped validation.
    pub fn resolved_document(&self) -> Result<String, String> {
        self.document_from(self.resolved_root()?)
    }

    /// Builds the canonical static document around a resolved root.
    fn document_from(&self, mut root: Value) -> Result<String, String> {
        hud_control::bind_selectors(&mut root);
        let namespace = self
            .screen
            .split_once('.')
            .ok_or("surface screen must be qualified")?
            .0;
        let name = self.screen.split_once('.').expect("qualified checked").1;
        let mut document = serde_json::Map::new();
        document.insert("namespace".into(), Value::String(namespace.into()));
        let mut grids = BTreeMap::new();
        for (name, collection) in &self.collections {
            collection.validate(name)?;
            let template: Value =
                serde_json::from_str(&collection.definition.template.resolved_document()?)
                    .map_err(|e| e.to_string())?;
            let source = collection
                .definition
                .template
                .screen
                .split_once('.')
                .expect("validated template")
                .1;
            let mut tile = template[source].clone();
            collection::scope(&mut tile, name);
            let template_name = format!("cinnabar_tile_{name}");
            if template_name == self.screen.split_once('.').expect("validated screen").1 {
                return Err("surface root collides with a native collection template".into());
            }
            let tile = serde_json::json!({"type":"panel","size":tile["size"],"clip_children":true,
                "bindings":[{"binding_type":"collection","binding_collection_name":name,
                    "binding_name":"#item_exists","binding_name_override":"#visible"}],
                "controls":[{"tile":tile}]});
            document.insert(template_name.clone(), tile);
            grids.insert(
                name.clone(),
                collection.grid(name, &format!("{namespace}.{template_name}")),
            );
        }
        collection::replace(&mut root, &mut grids, false)?;
        if !grids.is_empty() {
            return Err("surface collection is missing its placeholder".into());
        }
        document.insert(name.into(), root);
        Ok(Value::Object(document).to_string())
    }
    /// Collects capability-routed actions from the fully validated static tree.
    fn walk(&self, role: SurfaceRole) -> Result<Vec<String>, String> {
        self.walk_root(&self.resolved_root()?, role)
    }

    /// [`Self::walk`] over an already resolved root.
    fn walk_root(&self, root: &Value, role: SurfaceRole) -> Result<Vec<String>, String> {
        let mut count = 0;
        let mut actions = Vec::new();
        node(root, 0, &mut count, &mut actions, false, false)?;
        if role == SurfaceRole::Hud && renders(root, hud_control::RENDERER) {
            return Err("HUD surfaces cannot embed HUD controls".into());
        }
        if role == SurfaceRole::Hud && renders(root, backdrop::GLASS_RENDERER) {
            return Err("HUD surfaces cannot sample the world backdrop".into());
        }
        if self.collections.len() > 4 {
            return Err("surface has too many collections".into());
        }
        if serde_json::to_vec(&self.collections)
            .map_err(|e| e.to_string())?
            .len()
            > MAX_SURFACE_BYTES * 4
        {
            return Err("surface collection metadata exceeds its byte budget".into());
        }
        let mut grids = BTreeMap::new();
        for (name, collection) in &self.collections {
            collection.validate(name)?;
            let template: Value =
                serde_json::from_str(&collection.definition.template.resolved_document()?)
                    .map_err(|e| e.to_string())?;
            let source = collection
                .definition
                .template
                .screen
                .split_once('.')
                .expect("validated template")
                .1;
            count += collection
                .definition
                .items
                .len()
                .div_ceil(collection.definition.columns)
                * collection.definition.columns
                + (collection::count(&template[source]) + 1) * (collection::MAX_VISIBLE_CELLS + 1)
                + 8;
            if count > MAX_SURFACE_NODES {
                return Err("surface collections exceed the live node budget".into());
            }
            for item in &collection.definition.items {
                actions.extend(item.controls.values().map(|i| format!("mod.control:{i}")));
            }
            grids.insert(name.clone(), collection.grid(name, "unused.template"));
        }
        let mut placeholders = root.clone();
        collection::replace(&mut placeholders, &mut grids, false)?;
        if !grids.is_empty() {
            return Err("surface collection is missing its placeholder".into());
        }
        Ok(actions)
    }

    /// A surface rooted at `control`, a local definition of this document, carrying only the
    /// collections placed under it. It is checked as a HUD root is; bindings stay with `self`.
    pub fn embedded(&self, control: &str) -> Result<Surface, String> {
        self.embedded_document(control).map(|(surface, _)| surface)
    }

    /// [`Self::embedded`] and its canonical static document, resolving the control once.
    pub fn embedded_document(&self, control: &str) -> Result<(Surface, String), String> {
        if !hud_control::control_name(control) {
            return Err("embedded HUD control name is invalid".into());
        }
        let namespace = self
            .screen
            .split_once('.')
            .ok_or("surface screen must be qualified")?
            .0;
        let mut embedded = Surface {
            screen: format!("{namespace}.{control}"),
            document: self.document.clone(),
            bindings: BTreeMap::new(),
            transitions: BTreeMap::new(),
            collections: BTreeMap::new(),
        };
        let root = embedded.resolved_root()?;
        let mut placed = std::collections::BTreeSet::new();
        placeholders(&root, &mut placed);
        embedded.collections = self
            .collections
            .iter()
            .filter(|(name, _)| placed.contains(name.as_str()))
            .map(|(name, collection)| (name.clone(), collection.clone()))
            .collect();
        let actions = embedded.walk_root(&root, SurfaceRole::Hud)?;
        if !actions.is_empty() {
            return Err("HUD surfaces cannot declare input actions".into());
        }
        let document = embedded.document_from(root)?;
        Ok((embedded, document))
    }

    /// Compares immutable route artwork and data, excluding filtered order and reveal requests.
    pub fn same_definition(&self, other: &Self) -> bool {
        self.document == other.document && self.same_screen_and_collections(other)
    }

    /// [`Self::same_definition`] apart from the document's definitions.
    pub fn same_screen_and_collections(&self, other: &Self) -> bool {
        self.screen == other.screen
            && self.collections.len() == other.collections.len()
            && self.collections.iter().all(|(name, collection)| {
                other.collections.get(name).is_some_and(|other| {
                    Arc::ptr_eq(&collection.definition, &other.definition)
                        || collection.definition == other.definition
                })
            })
    }
    /// Resolves only static local templates before validating the emitted tree.
    fn resolved_root(&self) -> Result<Value, String> {
        self.resolved().map(|(root, _)| root)
    }

    /// [`Self::resolved_root`] and the document's parsed top-level members.
    fn resolved(&self) -> Result<(Value, serde_json::Map<String, Value>), String> {
        let object = self.static_definitions()?;
        Ok((self.resolve_static()?, object))
    }

    /// Checks the authored document before anything resolves it: its bytes, screen, local
    /// definitions, inheritance graph and expansion bound. Returns its parsed members.
    fn static_definitions(&self) -> Result<serde_json::Map<String, Value>, String> {
        if self.document.len() > MAX_SURFACE_BYTES {
            return Err("surface document exceeds byte limit".into());
        }
        let Some((namespace, name)) = self.screen.split_once('.') else {
            return Err("surface screen must be qualified".into());
        };
        if !identifier(namespace) || !identifier(name) {
            return Err("surface screen is invalid".into());
        }
        if matches!(namespace, "cinnabar_personal" | "cinnabar_hud_editor") {
            return Err("surface namespace is reserved by the host".into());
        }
        let document: Value = serde_json::from_str(&self.document)
            .map_err(|error| format!("invalid surface JSON: {error}"))?;
        let Value::Object(object) = document else {
            return Err("surface document must be an object".into());
        };
        if object.get("namespace").and_then(Value::as_str) != Some(namespace) {
            return Err("surface document must contain its own namespace".into());
        }
        let mut definitions = BTreeMap::new();
        for (key, body) in object.iter().filter(|(key, _)| *key != "namespace") {
            let (local, base) = local_reference(key, namespace)?;
            if definitions.insert(local, (body, base)).is_some() {
                return Err("duplicate surface definition".into());
            }
        }
        if !definitions.contains_key(name) {
            return Err("surface screen is missing".into());
        }
        let mut graph = BTreeMap::new();
        let mut authored_nodes = 0;
        for (key, (body, base)) in &definitions {
            let mut references = Vec::new();
            if let Some(base) = base {
                references.push(base.clone());
            }
            authored(body, namespace, 0, &mut authored_nodes, &mut references)?;
            graph.insert(key.clone(), references);
        }
        let references = |key: &str| graph.get(key).map(Vec::as_slice);
        for key in graph.keys() {
            acyclic(
                key,
                &references,
                &mut Vec::new(),
                &mut std::collections::BTreeSet::new(),
            )?;
        }
        bound_expansion(name, namespace, &definitions)?;
        Ok(object)
    }

    /// Resolves a document [`Self::static_definitions`] admitted and checks the emitted tree.
    fn resolve_static(&self) -> Result<Value, String> {
        let catalog = catalog(&self.document)?;
        let resolved = json_ui::resolve_with_limits(
            &catalog,
            &self.screen,
            &json_ui::Context::default(),
            33,
            MAX_SURFACE_NODES,
        );
        if !resolved.diagnostics.is_empty() {
            return Err(resolved.diagnostics.join("; "));
        }
        let root = resolved
            .control
            .as_ref()
            .ok_or("surface root cannot be resolved")?;
        let root = resolved_value(root)?;
        let mut count = 0;
        let mut actions = Vec::new();
        node(&root, 0, &mut count, &mut actions, false, false)?;
        Ok(root)
    }
}

/// Loads a surface document into a private catalog with paint transforms, rejecting any
/// load diagnostic.
fn catalog(document: &str) -> Result<json_ui::Catalog, String> {
    let files = br#"{"ui_defs":["ui/extension_surface.json"]}"#;
    let catalog = json_ui::Catalog::from_files([
        ("ui/_global_variables.json", &b"{}"[..]),
        ("ui/_ui_defs.json", &files[..]),
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

/// Adds every collection placeholder name under `value`.
fn placeholders(value: &Value, names: &mut std::collections::BTreeSet<String>) {
    if let Some(name) = value.get("cinnabar_collection").and_then(Value::as_str) {
        names.insert(name.to_owned());
    }
    for child in value
        .get("controls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
    {
        for body in child.values() {
            placeholders(body, names);
        }
    }
}

/// Whether `root` or any descendant draws through the custom `renderer`.
fn renders(root: &Value, renderer: &str) -> bool {
    root.get("renderer").and_then(Value::as_str) == Some(renderer)
        || root
            .get("controls")
            .and_then(Value::as_array)
            .is_some_and(|children| {
                children.iter().any(|child| {
                    child
                        .as_object()
                        .is_some_and(|child| child.values().any(|body| renders(body, renderer)))
                })
            })
}

/// Matches the names accepted by a local scalar data source.
pub(crate) fn binding_name(name: &str) -> bool {
    name.strip_prefix('#').is_some_and(identifier)
}

/// Restricts names to an unambiguous local identifier.
pub(crate) fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_BINDING_NAME_BYTES
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
}

/// Checks a finite authored tree without resolving factories or inheritance; `rotated`
/// reports an ancestor that may rotate its descendants and `filtered` one that filters them.
fn node(
    value: &Value,
    depth: usize,
    count: &mut usize,
    actions: &mut Vec<String>,
    rotated: bool,
    filtered: bool,
) -> Result<(), String> {
    *count += 1;
    if depth > 32 || *count > MAX_SURFACE_NODES {
        return Err("surface tree exceeds structural limits".into());
    }
    let object = value
        .as_object()
        .ok_or("surface control must be an object")?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or("surface control type is missing")?;
    if !matches!(
        kind,
        "screen"
            | "panel"
            | "input_panel"
            | "button"
            | "label"
            | "image"
            | "custom"
            | "stack_panel"
            | "scroll_view"
            | "scroll_track"
            | "scrollbar_box"
    ) || (kind == "screen" && depth != 0)
    {
        return Err("surface control type is unsupported".into());
    }
    if kind == "image" {
        local_player_head::validate(object)?;
    }
    for (name, value) in object {
        if name.starts_with('$') {
            return Err("surface variable declarations are unsupported".into());
        }
        if name != "controls" {
            property(value, 0)?;
        }
        if matches!(
            name.as_str(),
            "alpha" | "uv" | "color" | "clip_ratio" | "size" | "offset"
        ) && value.as_str().is_some_and(|text| text.starts_with('@'))
        {
            return Err("surface animation references are unsupported".into());
        }
    }
    for name in [
        "factory",
        "variables",
        "anims",
        "animations",
        "collection_name",
        "grid_dimensions",
        "property_bag",
        "property_bag_for_children",
    ] {
        if object.contains_key(name) {
            return Err("surface dynamic expansion is unsupported".into());
        }
    }
    let rotated = paint_transform::control(object, rotated)?;
    let filtered = backdrop::filter(object, filtered)?;
    if let Some(bindings) = object.get("bindings") {
        for binding in bindings
            .as_array()
            .ok_or("surface bindings must be an array")?
        {
            let binding = binding
                .as_object()
                .ok_or("surface binding must be an object")?;
            if binding
                .get("binding_type")
                .is_some_and(|kind| kind.as_str() != Some("global"))
            {
                return Err("surface bindings must use the typed global data source".into());
            }
        }
    }
    if kind == "custom"
        && !object
            .get("renderer")
            .and_then(Value::as_str)
            .is_some_and(|name| {
                matches!(
                    name,
                    json_ui::rounded::RENDERER
                        | backdrop::GLASS_RENDERER
                        | "cinnabar_vector_icon"
                        | "cinnabar_vector_path"
                        | "gradient_renderer"
                        | ITEM_RENDERER
                        | hud_control::RENDERER
                )
            })
    {
        return Err("surface custom renderer is unsupported".into());
    }
    if object.get("renderer").and_then(Value::as_str) == Some(backdrop::GLASS_RENDERER) {
        backdrop::validate_glass(object)?;
    }
    if object.get("renderer").and_then(Value::as_str) == Some(hud_control::RENDERER) {
        hud_control::validate(object)?;
    }
    if object.get("renderer").and_then(Value::as_str) == Some(json_ui::rounded::RENDERER) {
        shadow::validate(object)?;
    }
    if let Some(mappings) = object.get("button_mappings") {
        let mappings = mappings
            .as_array()
            .ok_or("surface mappings must be an array")?;
        if mappings.len() > 4 {
            return Err("surface has too many button mappings".into());
        }
        for mapping in mappings {
            if mapping.get("from_button_id").and_then(Value::as_str) != Some("button.menu_select")
                || mapping.get("mapping_type").and_then(Value::as_str) != Some("pressed")
            {
                return Err("surface buttons require pressed menu-select mappings".into());
            }
            let action = mapping
                .get("to_button_id")
                .and_then(Value::as_str)
                .ok_or("surface action is missing")?;
            if action.len() > 96 {
                return Err("surface action exceeds its bound".into());
            }
            if kind != "scroll_track" || action != SCROLL_TRACK_ACTION {
                actions.push(action.to_owned());
            }
        }
    }
    if let Some(children) = object.get("controls") {
        for entry in children
            .as_array()
            .ok_or("surface controls must be an array")?
        {
            let entry = entry.as_object().ok_or("surface child must be named")?;
            if entry.len() != 1 {
                return Err("surface child must have one name".into());
            }
            let (name, body) = entry.iter().next().expect("one member checked");
            if !identifier(name) || name.contains('@') {
                return Err("surface child inheritance is unsupported".into());
            }
            node(body, depth + 1, count, actions, rotated, filtered)?;
        }
    }
    Ok(())
}

/// Rejects structured aliases and inline animation graphs before JSON-UI substitution.
fn property(value: &Value, depth: usize) -> Result<(), String> {
    if depth > 16 {
        return Err("surface property exceeds structural limits".into());
    }
    match value {
        Value::String(text) if text.starts_with('$') => {
            return Err("surface variable references are unsupported".into());
        }
        Value::Array(items) => {
            for item in items {
                property(item, depth + 1)?;
            }
        }
        Value::Object(object) => {
            for (name, value) in object {
                if name.starts_with('$') || name == "anim_type" {
                    return Err(
                        "surface variable declarations and inline animations are unsupported"
                            .into(),
                    );
                }
                property(value, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
