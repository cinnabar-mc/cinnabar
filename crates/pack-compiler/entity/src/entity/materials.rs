//! Entity `.material` declarations, resolved as the Bedrock client resolves them.
//!
//! A file wrapped as `{"materials": {"version": "1.0.0", ...}}` is version 1; any other file is a
//! legacy version 0 file whose root holds the declarations. Version 0 keys only add to the
//! inherited material; from version 1 a plain key replaces and `+`/`-` keys add and remove.
//! A declaration's parent must come from the same file. Parents the client declares in its
//! own built-in groups resolve through [`builtin`], which stands in for that unshipped file; a
//! pack file that declares one of those names patches it for every file that inherits it.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use assets::{
    AssetError, EntityAssetSource, EntityRenderMaterialState,
    entity::material::{
        ENTITY_MATERIAL_SAMPLER_SLOTS, EntityMaterialBlendFactor, EntityMaterialCompare,
        EntityMaterialDefinition, EntityMaterialFilter, EntityMaterialMsaa,
        EntityMaterialPrimitive, EntityMaterialSampler, EntityMaterialState,
        EntityMaterialStencilFace, EntityMaterialStencilOperation, EntityMaterialVariant,
        EntityMaterialVertexField, EntityMaterialWrap, MAX_ENTITY_MATERIAL_DEFINITIONS,
        MAX_ENTITY_MATERIAL_NAME_BYTES,
    },
};
use serde_json::{Map, Value};

use super::SourceDocuments;

/// Deepest variant nesting resolved before the remainder is dropped.
const MAX_VARIANT_DEPTH: usize = 8;
/// Longest parent chain resolved before the material is treated as cyclic.
const MAX_PARENT_DEPTH: usize = 64;

/// One `+name` declaration: its file's version and fields.
type Modification = (u8, Map<String, Value>);

struct Declaration {
    file: usize,
    version: u8,
    parent: Option<Box<str>>,
    fields: Map<String, Value>,
}

#[derive(Default)]
pub(super) struct MaterialStates {
    files: usize,
    definitions: BTreeMap<Box<str>, Declaration>,
    /// `+name` declarations applied in place after the named material resolves.
    modifications: BTreeMap<Box<str>, Vec<Modification>>,
}

/// One material during resolution, before its variants are interned into a carrier table.
#[derive(Clone, Debug, Default, PartialEq)]
struct Resolved {
    version: u8,
    states: BTreeSet<EntityMaterialState>,
    defines: BTreeSet<Box<str>>,
    definition: EntityMaterialDefinition,
    samplers: [Option<EntityMaterialSampler>; ENTITY_MATERIAL_SAMPLER_SLOTS as usize],
    variants: Vec<(Box<str>, Resolved)>,
}

/// Interns resolved definitions, including variants, into one carrier table.
#[derive(Default)]
pub(super) struct MaterialTable {
    definitions: Vec<EntityMaterialDefinition>,
}

impl MaterialTable {
    /// The finished table, in index order.
    pub(super) fn into_definitions(self) -> Box<[EntityMaterialDefinition]> {
        self.definitions.into_boxed_slice()
    }

    /// Interns `resolved` and its variants, reusing an equal existing entry.
    fn intern(&mut self, resolved: &Resolved) -> Result<u32, AssetError> {
        let mut definition = resolved.definition.clone();
        definition.variants = resolved
            .variants
            .iter()
            .map(|(name, variant)| {
                Ok(EntityMaterialVariant {
                    name: name.clone(),
                    definition: self.intern(variant)?,
                })
            })
            .collect::<Result<_, AssetError>>()?;
        if let Some(index) = self
            .definitions
            .iter()
            .position(|existing| *existing == definition)
        {
            return Ok(index as u32);
        }
        if self.definitions.len() >= MAX_ENTITY_MATERIAL_DEFINITIONS {
            return Err(super::invalid(
                "entity material definitions exceed admitted bound",
            ));
        }
        self.definitions.push(definition);
        Ok(self.definitions.len() as u32 - 1)
    }
}

impl MaterialStates {
    /// Reads retained material files once for this compilation, in path order.
    pub(super) fn load(
        root: &Path,
        payloads: &SourceDocuments<'_>,
        sources: &[EntityAssetSource],
    ) -> Result<Self, AssetError> {
        let mut paths = sources
            .iter()
            .filter(|source| {
                source.path.starts_with("materials/") && source.path.ends_with(".material")
            })
            .map(|source| source.path.as_ref())
            .collect::<Vec<_>>();
        paths.sort_unstable();
        let mut states = Self::default();
        for path in paths {
            states.read(payloads.read(root, path)?);
        }
        Ok(states)
    }

    /// Admits one file. Without a `+` prefix the first declaration of a name wins.
    pub(super) fn read(&mut self, value: &Value) {
        let file = self.files;
        self.files += 1;
        let (entries, version) = match file_entries(value) {
            Some(entries) => entries,
            None => return,
        };
        for (declaration, value) in entries {
            let Some(fields) = value.as_object() else {
                continue;
            };
            let (declaration, modify) = match declaration.strip_prefix('+') {
                Some(declaration) => (declaration, true),
                None => (declaration.as_str(), false),
            };
            let (name, parent) = declaration
                .rsplit_once(':')
                .map_or((declaration, None), |(name, parent)| {
                    (name, Some(parent.into()))
                });
            if name.is_empty() || name.len() > MAX_ENTITY_MATERIAL_NAME_BYTES {
                continue;
            }
            if modify {
                self.modifications
                    .entry(name.into())
                    .or_default()
                    .push((version, fields.clone()));
                continue;
            }
            if self.definitions.len() >= assets::MAX_ENTITY_ASSET_SYMBOLS {
                continue;
            }
            self.definitions
                .entry(name.into())
                .or_insert_with(|| Declaration {
                    file,
                    version,
                    parent,
                    fields: fields.clone(),
                });
        }
    }

    /// Replaces lower declarations with a separately admitted layer and returns its names.
    pub(super) fn overlay(&mut self, upper: Self) -> BTreeSet<String> {
        let offset = self.files;
        let names = upper
            .definitions
            .keys()
            .chain(upper.modifications.keys())
            .map(ToString::to_string)
            .collect();
        for (name, mut declaration) in upper.definitions {
            declaration.file += offset;
            self.definitions.insert(name, declaration);
        }
        for (name, modifications) in upper.modifications {
            self.modifications
                .entry(name)
                .or_default()
                .extend(modifications);
        }
        self.files += upper.files;
        names
    }

    /// Includes the same normalized names and parent links used by material resolution.
    pub(super) fn extend_dependencies(&self, names: &mut BTreeSet<String>) {
        let mut pending = names
            .iter()
            .map(|name| material_name(name).to_owned())
            .collect::<Vec<_>>();
        let mut seen = BTreeSet::new();
        while let Some(name) = pending.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            names.insert(name.clone());
            if let Some(parent) = self
                .definitions
                .get(name.as_str())
                .and_then(|declaration| declaration.parent.as_deref())
            {
                pending.push(parent.to_owned());
            }
        }
    }

    /// Resolves the dye mask from `USE_COLOR_MASK`, keeping an unknown parent unresolved.
    pub(super) fn color_mask(&self, target: &str) -> assets::EquipmentColorMask {
        use assets::EquipmentColorMask;
        match self.resolve_with(material_name(target), &color_mask_base) {
            Some(resolved) if resolved.defines.contains(COLOR_MASK) => EquipmentColorMask::Dye,
            Some(_) => EquipmentColorMask::NoMask,
            None => EquipmentColorMask::Unresolved,
        }
    }

    /// Resolves `target`, interns its complete definition and returns its shading features
    /// alongside the definition's table index.
    pub(super) fn resolve(
        &self,
        target: &str,
        table: &mut MaterialTable,
    ) -> Result<Option<(EntityRenderMaterialState, u32)>, AssetError> {
        let Some(resolved) = self.resolve_with(material_name(target), &builtin) else {
            return Ok(None);
        };
        Ok(Some((shading(&resolved), table.intern(&resolved)?)))
    }

    fn resolve_with(
        &self,
        name: &str,
        base: &dyn Fn(&str) -> Option<Resolved>,
    ) -> Option<Resolved> {
        self.resolve_depth(name, None, base, 0)
    }

    /// Resolves `name`; `file` restricts declared parents to the child's own file.
    fn resolve_depth(
        &self,
        name: &str,
        file: Option<usize>,
        base: &dyn Fn(&str) -> Option<Resolved>,
        depth: usize,
    ) -> Option<Resolved> {
        if depth > MAX_PARENT_DEPTH {
            return None;
        }
        let Some(declaration) = self
            .definitions
            .get(name)
            .filter(|declaration| file.is_none_or(|file| declaration.file == file))
        else {
            // A parent declared only in another pack file is unreachable, as on the client,
            // unless the client also has a built-in base of that name: the pack's declaration of
            // it then applies, exactly as for a direct reference to that name.
            if self.definitions.contains_key(name) {
                if file.is_some() && base(name).is_some() {
                    return self.resolve_depth(name, None, base, depth + 1);
                }
                return None;
            }
            return base(name);
        };
        let mut resolved = match declaration.parent.as_deref() {
            Some(parent) => self.resolve_depth(parent, Some(declaration.file), base, depth + 1)?,
            None => base(name).unwrap_or_default(),
        };
        if !lists_are_well_formed(&declaration.fields)
            || self
                .modifications
                .get(name)
                .into_iter()
                .flatten()
                .any(|(_, fields)| !lists_are_well_formed(fields))
        {
            return None;
        }
        resolved.version = declaration.version;
        resolved.apply(&declaration.fields, 0);
        for (version, fields) in self.modifications.get(name).into_iter().flatten() {
            resolved.version = *version;
            resolved.apply(fields, 0);
        }
        Some(resolved)
    }
}

/// The declarations of one file and the version they parse with.
fn file_entries(value: &Value) -> Option<(&Map<String, Value>, u8)> {
    let root = value.as_object()?;
    if root.len() == 1
        && let Some(materials) = root.get("materials").and_then(Value::as_object)
        && materials.get("version").and_then(Value::as_str) == Some("1.0.0")
    {
        return Some((materials, 1));
    }
    Some((root, 0))
}

/// Skinning variants use their base material's declaration.
fn material_name(target: &str) -> &str {
    target.strip_suffix(".skinning").unwrap_or(target)
}

const OVERLAY: &str = "USE_OVERLAY";
const ALPHA_TEST: &str = "ALPHA_TEST";
const EMISSIVE: &str = "USE_EMISSIVE";
const COLOR_MASK: &str = "USE_COLOR_MASK";
const MULTITEXTURE: &str = "USE_MULTITEXTURE";

/// Native material bases the client ships outside resource packs, as the states, defines and
/// colour blend pair each declares. The generated native table wins over the base names below.
fn builtin(name: &str) -> Option<Resolved> {
    use EntityMaterialState as State;
    use assets::facts::pack::MaterialKind;
    let rules = assets::facts::pack::rules();
    if let Some(state) = rules.material_state(name) {
        let mut resolved = Resolved::from_definition(state.definition());
        // Native dyed and three-sampler bases declare their texture-combining define.
        match rules.material(name) {
            Some(MaterialKind::ColorMask) => {
                resolved.defines.insert(COLOR_MASK.into());
            }
            Some(MaterialKind::Multitexture) => {
                resolved.defines.insert(MULTITEXTURE.into());
            }
            _ => {}
        }
        resolved.finish();
        return Some(resolved);
    }
    let (states, defines): (&[State], &[&str]) = match name {
        "entity" => (&[], &[OVERLAY]),
        "entity_static" => (&[], &[]),
        "entity_nocull" => (&[State::DisableCulling], &[OVERLAY]),
        "entity_alphatest_one_sided" => (&[], &[ALPHA_TEST, OVERLAY]),
        "entity_emissive" => (&[], &[EMISSIVE, OVERLAY]),
        "entity_emissive_alpha" => (&[State::DisableCulling], &[EMISSIVE, ALPHA_TEST, OVERLAY]),
        "entity_emissive_alpha_one_sided" => (&[], &[EMISSIVE, ALPHA_TEST, OVERLAY]),
        "entity_alphablend" | "entity_alphablend_nocolor" => (&[State::Blending], &[OVERLAY]),
        _ => return None,
    };
    let mut resolved = Resolved::default();
    resolved.states.extend(states.iter().copied());
    resolved
        .defines
        .extend(defines.iter().map(|&define| Box::from(define)));
    resolved.finish();
    Some(resolved)
}

/// Bases for dye masks: leather armor masks, plain armor and other native bases do not.
fn color_mask_base(name: &str) -> Option<Resolved> {
    match name {
        "armor_leather" | "armor_leather_enchanted" => {
            let mut resolved = Resolved::default();
            resolved.defines.insert(COLOR_MASK.into());
            Some(resolved)
        }
        "armor" | "armor_enchanted" => Some(Resolved::default()),
        _ if assets::facts::pack::rules().material(name)
            == Some(assets::facts::pack::MaterialKind::UnmaskedArmor) =>
        {
            Some(Resolved::default())
        }
        _ => builtin(name),
    }
}

/// The shading features the actor shader reads from a definition's defines.
fn shading(resolved: &Resolved) -> EntityRenderMaterialState {
    EntityRenderMaterialState {
        // Native JSON actors select the transparent pass before legacy ALPHA_TEST; that pass
        // preserves sampled opacity without a cutout discard.
        alpha_test: resolved.defines.contains(ALPHA_TEST)
            && !resolved.states.contains(&EntityMaterialState::Blending),
        emissive: resolved.defines.contains(EMISSIVE),
        only_emissive: resolved.defines.contains("USE_ONLY_EMISSIVE"),
        disable_overlay: !resolved.defines.contains(OVERLAY),
        color_mask: resolved.defines.contains(COLOR_MASK),
        multitexture: resolved.defines.contains(MULTITEXTURE),
    }
}

impl Resolved {
    /// Working sets for a complete definition without variants or samplers.
    fn from_definition(definition: EntityMaterialDefinition) -> Self {
        Self {
            states: definition.states.iter().copied().collect(),
            defines: definition.defines.iter().cloned().collect(),
            definition,
            ..Self::default()
        }
    }

    /// Applies one declaration's fields with this material's version rules.
    fn apply(&mut self, fields: &Map<String, Value>, depth: usize) {
        let modern = self.version >= 1;
        names(
            fields,
            "states",
            modern,
            &mut self.states,
            EntityMaterialState::from_name,
        );
        names(fields, "defines", modern, &mut self.defines, |name| {
            (!name.is_empty() && name.len() <= MAX_ENTITY_MATERIAL_NAME_BYTES).then(|| name.into())
        });
        if self
            .states
            .contains(&EntityMaterialState::EnableAlphaToCoverage)
        {
            self.defines.insert("ALPHA_TO_COVERAGE".into());
        }
        let definition = &mut self.definition;
        for (key, slot) in [
            ("vertexShader", &mut definition.vertex_shader),
            ("fragmentShader", &mut definition.fragment_shader),
            ("geometryShader", &mut definition.geometry_shader),
        ] {
            if let Some(name) =
                text(fields, key).filter(|name| name.len() <= MAX_ENTITY_MATERIAL_NAME_BYTES)
            {
                *slot = Some(name.into());
            }
        }
        if let Some(value) = text(fields, "depthFunc").and_then(EntityMaterialCompare::from_name) {
            definition.depth_func = value;
        }
        for (key, face) in [
            ("frontFace", &mut definition.front_face),
            ("backFace", &mut definition.back_face),
        ] {
            if let Some(fields) = fields.get(key).and_then(Value::as_object) {
                stencil_face(fields, face);
            }
        }
        if let Some(reference) = fields.get("stencilRef").and_then(Value::as_u64) {
            definition.stencil_ref = Some(reference as u8);
        }
        for (key, slot) in [
            ("stencilReadMask", &mut definition.stencil_read_mask),
            ("stencilWriteMask", &mut definition.stencil_write_mask),
        ] {
            if let Some(mask) = fields.get(key).and_then(Value::as_u64) {
                *slot = mask as u32;
            }
        }
        let factor = |key| text(fields, key).and_then(EntityMaterialBlendFactor::from_name);
        if let Some(source) = factor("blendSrc") {
            definition.blend[0] = source;
        }
        if let Some(destination) = factor("blendDst") {
            definition.blend[1] = destination;
        }
        // An absent alpha source follows the colour source; an absent alpha destination
        // follows only a colour destination authored in the same declaration.
        definition.blend[2] = factor("alphaSrc").unwrap_or(definition.blend[0]);
        if let Some(destination) = factor("alphaDst").or_else(|| factor("blendDst")) {
            definition.blend[3] = destination;
        }
        if let Some(value) = finite_f32(fields, "depthBias") {
            definition.depth_bias = value;
        }
        if let Some(value) = finite_f32(fields, "slopeScaledDepthBias") {
            definition.slope_scaled_depth_bias = value;
        }
        if let Some(value) = text(fields, "msaaSupport").and_then(EntityMaterialMsaa::from_name) {
            definition.msaa = value;
        }
        if let Some(value) =
            text(fields, "primitiveMode").and_then(EntityMaterialPrimitive::from_name)
        {
            definition.primitive = value;
        }
        if let Some(values) = fields.get("vertexFields").and_then(Value::as_array) {
            definition.vertex_fields = Some(
                values
                    .iter()
                    .filter_map(|value| value.get("field")?.as_str())
                    .filter_map(EntityMaterialVertexField::from_name)
                    .collect(),
            );
        }
        self.apply_samplers(fields, modern);
        if depth < MAX_VARIANT_DEPTH {
            self.apply_variants(fields, modern, depth);
        }
        self.finish();
    }

    fn apply_samplers(&mut self, fields: &Map<String, Value>, modern: bool) {
        let plain = fields.get("samplerStates").and_then(Value::as_array);
        if modern && plain.is_some() {
            self.samplers = Default::default();
        }
        let entries = plain.or_else(|| fields.get("+samplerStates").and_then(Value::as_array));
        for entry in entries.into_iter().flatten().filter_map(Value::as_object) {
            let Some(slot) = entry
                .get("samplerIndex")
                .and_then(Value::as_u64)
                .filter(|slot| *slot < u64::from(ENTITY_MATERIAL_SAMPLER_SLOTS))
            else {
                continue;
            };
            let stages = match text(entry, "shaderStage") {
                Some("Vertex") => 1,
                Some("Geometry") => 4,
                _ => 2,
            };
            self.samplers[slot as usize] = Some(EntityMaterialSampler {
                slot: slot as u8,
                filter: text(entry, "textureFilter")
                    .and_then(EntityMaterialFilter::from_name)
                    .unwrap_or_default(),
                wrap: text(entry, "textureWrap")
                    .and_then(EntityMaterialWrap::from_name)
                    .unwrap_or_default(),
                stages,
            });
        }
    }

    /// Each variant clones the material parsed so far, then applies its own fields. Version 1
    /// replaces inherited variants with a plain list; otherwise it adds `+variants` and then
    /// removes the inherited or added variants that `-variants` names.
    fn apply_variants(&mut self, fields: &Map<String, Value>, modern: bool, depth: usize) {
        let plain = fields.get("variants").and_then(Value::as_array);
        if modern && plain.is_some() {
            self.variants.clear();
        }
        self.add_variants(fields, plain, depth);
        if modern && plain.is_none() {
            let removed = fields.get("-variants").and_then(Value::as_array);
            for name in removed.into_iter().flatten().filter_map(Value::as_str) {
                self.variants
                    .retain(|(existing, _)| existing.as_ref() != name);
            }
        }
    }

    /// Clones and applies each entry of the plain list, or of `+variants` when it is absent,
    /// replacing an existing variant of the same name.
    fn add_variants(
        &mut self,
        fields: &Map<String, Value>,
        plain: Option<&Vec<Value>>,
        depth: usize,
    ) {
        let entries = plain.or_else(|| fields.get("+variants").and_then(Value::as_array));
        for entry in entries.into_iter().flatten().filter_map(Value::as_object) {
            for (name, value) in entry {
                let Some(variant_fields) = value.as_object() else {
                    continue;
                };
                if name.is_empty() || name.len() > MAX_ENTITY_MATERIAL_NAME_BYTES {
                    continue;
                }
                let mut variant = self.clone();
                variant.variants.clear();
                if let Some(version) = text(variant_fields, "version") {
                    variant.version = u8::from(!version.starts_with('0'));
                }
                variant.apply(variant_fields, depth + 1);
                self.variants
                    .retain(|(existing, _)| existing.as_ref() != name);
                self.variants.push((name.as_str().into(), variant));
            }
        }
    }

    /// Copies the working sets into the canonical carrier fields.
    fn finish(&mut self) {
        self.definition.states = self.states.iter().copied().collect();
        self.definition.defines = self.defines.iter().cloned().collect();
        self.definition.samplers = self.samplers.iter().flatten().copied().collect();
    }
}

/// Whether every present state and define list is an array of strings. A malformed list leaves
/// the material unresolved rather than guessing which entries were meant.
fn lists_are_well_formed(fields: &Map<String, Value>) -> bool {
    [
        "states", "+states", "-states", "defines", "+defines", "-defines",
    ]
    .into_iter()
    .filter_map(|key| fields.get(key).filter(|value| !value.is_null()))
    .all(|value| {
        value
            .as_array()
            .is_some_and(|values| values.iter().all(Value::is_string))
    })
}

/// A number that stays finite once narrowed to `f32`; one that overflows is treated as absent,
/// so the inherited value remains instead of an unrepresentable bias.
fn finite_f32(fields: &Map<String, Value>, key: &str) -> Option<f32> {
    fields
        .get(key)
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .filter(|value| value.is_finite())
}

fn text<'a>(fields: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    fields.get(key).and_then(Value::as_str)
}

/// Applies `key`, `+key` and `-key` lists with the version's replace and add rules.
fn names<T: Ord>(
    fields: &Map<String, Value>,
    key: &str,
    modern: bool,
    set: &mut BTreeSet<T>,
    parse: impl Fn(&str) -> Option<T>,
) {
    let list = |key: &str| {
        fields.get(key).and_then(Value::as_array).map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .filter_map(&parse)
                .collect::<Vec<_>>()
        })
    };
    if let Some(values) = list(key) {
        if modern {
            set.clear();
        }
        set.extend(values);
        return;
    }
    set.extend(list(&format!("+{key}")).into_iter().flatten());
    if modern {
        for value in list(&format!("-{key}")).into_iter().flatten() {
            set.remove(&value);
        }
    }
}

fn stencil_face(fields: &Map<String, Value>, face: &mut EntityMaterialStencilFace) {
    if let Some(compare) = text(fields, "stencilFunc").and_then(EntityMaterialCompare::from_name) {
        face.compare = compare;
    }
    for (key, slot) in [
        ("stencilFailOp", &mut face.fail),
        ("stencilDepthFailOp", &mut face.depth_fail),
        ("stencilPassOp", &mut face.pass),
    ] {
        if let Some(operation) =
            text(fields, key).and_then(EntityMaterialStencilOperation::from_name)
        {
            *slot = operation;
        }
    }
}

#[cfg(test)]
#[path = "materials/tests.rs"]
mod tests;
