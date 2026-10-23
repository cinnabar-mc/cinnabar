//! Entity `.material` declarations, resolved as the Bedrock client resolves them.
//!
//! A file wrapped as `{"materials": {"version": "1.0.0", ...}}` is version 1; any other file is a
//! legacy version 0 file whose root holds the declarations. Version 0 keys only add to the
//! inherited material; from version 1 a plain key replaces and `+`/`-` keys add and remove.
//! A declaration's parent must come from the same file. Parents the client declares in its
//! own built-in groups resolve through [`builtin`], which stands in for that unshipped file; a
//! pack file that declares one of those names patches it for every file that inherits it.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::Path,
    sync::Arc,
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
    /// The pack's `cinnabar/material-bindings.json`, when valid.
    extension: Option<extension::Extension>,
    files: usize,
    definitions: BTreeMap<Box<str>, Declaration>,
    /// `+name` declarations applied in place after the named material resolves.
    modifications: BTreeMap<Box<str>, Vec<Modification>>,
    /// Declarations past the symbol bound, which stay unresolved.
    unadmitted: u32,
}

/// One material during resolution, before its variants are interned into a carrier table.
/// Every field whose size an author controls is shared and copied only when a declaration
/// changes it, so a link that inherits it unchanged costs a reference; the rest is bounded.
#[derive(Clone, Debug, Default, PartialEq)]
struct Resolved {
    version: u8,
    states: Arc<BTreeSet<EntityMaterialState>>,
    defines: Arc<BTreeSet<Box<str>>>,
    /// `None` keeps the default vertex layout.
    vertex_fields: Option<Arc<[EntityMaterialVertexField]>>,
    /// Bounded fields only; [`Self::carrier`] adds the shared lists.
    definition: EntityMaterialDefinition,
    samplers: [Option<EntityMaterialSampler>; ENTITY_MATERIAL_SAMPLER_SLOTS as usize],
    /// Shared until a declaration changes the list, so links that inherit variants unchanged
    /// hold one allocation.
    variants: Arc<Vec<(Box<str>, Resolved)>>,
}

/// Interns resolved definitions, including variants, into one carrier table. One table serves
/// one [`MaterialStates`], whose resolutions it memoises by name.
#[derive(Default)]
pub(super) struct MaterialTable {
    definitions: Vec<EntityMaterialDefinition>,
    /// Table index of each interned definition.
    indices: HashMap<EntityMaterialDefinition, u32>,
    /// Shading features and table index already resolved for a material name.
    resolved: HashMap<Box<str>, Option<(EntityRenderMaterialState, u32)>>,
    /// Definitions left unresolved because the table reached its carrier bound.
    unresolved: u32,
    /// Parent-chain links resolved against the built-in bases.
    chains: ChainMemo,
}

impl MaterialTable {
    /// The finished table, in index order.
    pub(super) fn into_definitions(self) -> Box<[EntityMaterialDefinition]> {
        self.definitions.into_boxed_slice()
    }

    /// Definitions and variants that did not fit the carrier bound. Their materials draw as
    /// layers without one, and the rest of the pack still compiles.
    pub(super) fn unresolved(&self) -> u32 {
        self.unresolved
    }

    /// Interns `resolved` and its variants, reusing an equal existing entry. Recursion depth
    /// follows variant nesting, which the JSON parser's recursion limit bounds. A definition past
    /// the carrier bound stays unresolved and is counted; a variant that does not fit is left
    /// out of its owner.
    fn intern(&mut self, resolved: &Resolved) -> Option<u32> {
        let mut definition = resolved.carrier();
        definition.variants = resolved
            .variants
            .iter()
            .filter_map(|(name, variant)| {
                Some(EntityMaterialVariant {
                    name: name.clone(),
                    definition: self.intern(variant)?,
                })
            })
            .collect();
        if let Some(&index) = self.indices.get(&definition) {
            return Some(index);
        }
        if self.definitions.len() >= MAX_ENTITY_MATERIAL_DEFINITIONS {
            self.unresolved += 1;
            return None;
        }
        let index = self.definitions.len() as u32;
        self.indices.insert(definition.clone(), index);
        self.definitions.push(definition);
        Some(index)
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
        if sources
            .iter()
            .any(|source| source.path.as_ref() == extension::MATERIAL_BINDINGS_PATH)
        {
            states.extension =
                extension::parse(payloads.read(root, extension::MATERIAL_BINDINGS_PATH)?).ok();
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
            if self.definitions.len() >= assets::MAX_ENTITY_ASSET_SYMBOLS
                && !self.definitions.contains_key(name)
            {
                self.unadmitted += 1;
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
        self.unadmitted += upper.unadmitted;
        names
    }

    /// Declarations left out because the file set declared more materials than the symbol
    /// bound admits. Their materials draw as layers without one.
    pub(super) fn unadmitted(&self) -> u32 {
        self.unadmitted
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
    /// `links` is shared across one compile's lookups and serves only this base.
    pub(super) fn color_mask(
        &self,
        target: &str,
        links: &mut ChainMemo,
    ) -> assets::EquipmentColorMask {
        use assets::EquipmentColorMask;
        match self.resolve_with(material_name(target), &color_mask_base, links) {
            Some(resolved) if resolved.defines.contains(COLOR_MASK) => EquipmentColorMask::Dye,
            Some(_) => EquipmentColorMask::NoMask,
            None => EquipmentColorMask::Unresolved,
        }
    }

    /// Resolves `target`, interns its complete definition and returns its shading features
    /// alongside the definition's table index. `None` leaves the material unresolved.
    pub(super) fn resolve(
        &self,
        target: &str,
        table: &mut MaterialTable,
    ) -> Option<(EntityRenderMaterialState, u32)> {
        let name = material_name(target);
        if let Some(resolved) = table.resolved.get(name) {
            return *resolved;
        }
        let resolved = self
            .resolve_with(name, &builtin, &mut table.chains)
            .and_then(|resolved| Some((shading(&resolved), table.intern(&resolved)?)));
        table.resolved.insert(name.into(), resolved);
        resolved
    }

    /// Resolves a pack's block bindings: `.material` documents in path order supply bases, and
    /// bindings whose material does not resolve are dropped.
    pub(crate) fn block_bindings(
        material_documents: &[Value],
        document: &Value,
    ) -> Vec<extension::ResolvedBlockBinding> {
        let mut states = Self::default();
        for value in material_documents {
            states.read(value);
        }
        states.extension = extension::parse(document).ok();
        let mut table = MaterialTable::default();
        let Some(extension) = &states.extension else {
            return Vec::new();
        };
        extension
            .block_bindings
            .iter()
            .filter_map(|binding| {
                let (shading, index) = states.resolve_extension(&binding.material, &mut table)?;
                Some(extension::ResolvedBlockBinding {
                    block: binding.block.clone(),
                    states: binding.states.clone(),
                    material_instance: binding.material_instance.clone(),
                    definition: table.definitions[index as usize].clone(),
                    shading,
                })
            })
            .collect()
    }

    /// Extension material bindings of `entity`'s `controller`, as (bone pattern, material), in
    /// document order.
    pub(super) fn extension_rules<'a>(
        &'a self,
        entity: &'a str,
        controller: &'a str,
    ) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
        self.extension
            .iter()
            .flat_map(move |extension| extension.entity_rules(entity, controller))
    }

    /// Resolves extension material `name` over its base, interns it and returns its shading
    /// features with the definition's table index.
    pub(super) fn resolve_extension(
        &self,
        name: &str,
        table: &mut MaterialTable,
    ) -> Option<(EntityRenderMaterialState, u32)> {
        let material = self.extension.as_ref()?.materials.get(name)?;
        let mut resolved = match &material.base {
            Some(base) => self.resolve_with(material_name(base), &builtin, &mut table.chains)?,
            None => Resolved::default(),
        };
        resolved.version = 1;
        resolved.apply(&material.fields);
        Some((shading(&resolved), table.intern(&resolved)?))
    }

    /// Walks `name`'s parent chain until a link `memo` already knows, then applies each
    /// declaration and its `+name` modifications from there down, remembering every link, so
    /// each declaration applies once however many names share its chain. Declared parents come
    /// from the child's own file. A chain that returns to a link it already visited is a cycle,
    /// which the client's inheritance tree never reaches, so it stays unresolved; there is no
    /// other depth bound. `memo` must only ever be used with the same `base`.
    fn resolve_with(
        &self,
        name: &str,
        base: &dyn Fn(&str) -> Option<Resolved>,
        memo: &mut ChainMemo,
    ) -> Option<Resolved> {
        enum Link<'a> {
            Declared(&'a Declaration),
            /// A parent from another file resolved through the pack's own declaration of a
            /// built-in base; it resolves exactly as the next link.
            Retried,
        }
        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        let (mut current, mut file) = (name, None);
        let mut resolved = loop {
            if let Some(known) = memo.links.get(&(Box::from(current), file)) {
                break known.as_deref().cloned();
            }
            if !visited.insert((current, file)) {
                break None;
            }
            let Some(declaration) = self
                .definitions
                .get(current)
                .filter(|declaration| file.is_none_or(|file| declaration.file == file))
            else {
                // A parent declared only in another pack file is unreachable, as on the
                // client, unless the client also has a built-in base of that name: the pack's
                // declaration of it then applies, exactly as for a direct reference to that
                // name.
                if self.definitions.contains_key(current) {
                    if file.is_some() && base(current).is_some() {
                        chain.push((current, file, Link::Retried));
                        file = None;
                        continue;
                    }
                    break None;
                }
                // A built-in base the pack never declares still takes its modifications.
                let resolved = base(current).and_then(|mut resolved| {
                    self.modify(current, &mut resolved, &mut memo.applies)?;
                    Some(resolved)
                });
                memo.links
                    .insert((current.into(), file), resolved.clone().map(Arc::new));
                break resolved;
            };
            // A direct reference and a parent link reach the same declaration alike.
            if file.is_none()
                && let Some(known) = memo
                    .links
                    .get(&(Box::from(current), Some(declaration.file)))
            {
                let known = known.clone();
                let resolved = known.as_deref().cloned();
                memo.links.insert((current.into(), None), known);
                break resolved;
            }
            chain.push((current, file, Link::Declared(declaration)));
            match declaration.parent.as_deref() {
                Some(parent) => {
                    file = Some(declaration.file);
                    current = parent;
                }
                None => break Some(base(current).unwrap_or_default()),
            }
        };
        for (name, file, link) in chain.into_iter().rev() {
            if let Link::Declared(declaration) = link {
                resolved = resolved.and_then(|mut resolved| {
                    if !lists_are_well_formed(&declaration.fields) {
                        return None;
                    }
                    resolved.version = declaration.version;
                    resolved.apply(&declaration.fields);
                    memo.applies += 1;
                    self.modify(name, &mut resolved, &mut memo.applies)?;
                    Some(resolved)
                });
                // A parent link reaches this declaration under its file's key.
                let shared = resolved.clone().map(Arc::new);
                memo.links
                    .insert((name.into(), Some(declaration.file)), shared.clone());
                memo.links.insert((name.into(), file), shared);
                continue;
            }
            memo.links
                .insert((name.into(), file), resolved.clone().map(Arc::new));
        }
        resolved
    }

    /// Applies `name`'s `+name` modifications in order, counting each in `applies`; a
    /// malformed list in any of them leaves the material unresolved.
    fn modify(&self, name: &str, resolved: &mut Resolved, applies: &mut usize) -> Option<()> {
        let modifications = self.modifications.get(name).map_or(&[][..], Vec::as_slice);
        if modifications
            .iter()
            .any(|(_, fields)| !lists_are_well_formed(fields))
        {
            return None;
        }
        for (version, fields) in modifications {
            resolved.version = *version;
            resolved.apply(fields);
            *applies += 1;
        }
        Some(())
    }
}

/// A walked parent-chain link: a material name and the declaring file it is restricted to.
type ChainLink = (Box<str>, Option<usize>);

/// Resolutions of each walked (name, declaring-file restriction) link for one base, so a
/// parent chain shared by many names resolves once.
#[derive(Default)]
pub(super) struct ChainMemo {
    /// One shared resolution per link; a link stored under two keys holds one allocation.
    links: HashMap<ChainLink, Option<Arc<Resolved>>>,
    /// Declarations and modifications applied so far; tests bound resolution work with it.
    applies: usize,
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
                Arc::make_mut(&mut resolved.defines).insert(COLOR_MASK.into());
            }
            Some(MaterialKind::Multitexture) => {
                Arc::make_mut(&mut resolved.defines).insert(MULTITEXTURE.into());
            }
            _ => {}
        }
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
    Some(Resolved {
        states: Arc::new(states.iter().copied().collect()),
        defines: Arc::new(defines.iter().map(|&define| Box::from(define)).collect()),
        ..Resolved::default()
    })
}

/// Bases for dye masks: leather armor masks, plain armor and other native bases do not.
fn color_mask_base(name: &str) -> Option<Resolved> {
    match name {
        "armor_leather" | "armor_leather_enchanted" => {
            let mut resolved = Resolved::default();
            Arc::make_mut(&mut resolved.defines).insert(COLOR_MASK.into());
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
    fn from_definition(mut definition: EntityMaterialDefinition) -> Self {
        Self {
            states: Arc::new(
                std::mem::take(&mut definition.states)
                    .into_vec()
                    .into_iter()
                    .collect(),
            ),
            defines: Arc::new(
                std::mem::take(&mut definition.defines)
                    .into_vec()
                    .into_iter()
                    .collect(),
            ),
            vertex_fields: definition.vertex_fields.take().map(Arc::from),
            definition,
            ..Self::default()
        }
    }

    /// The complete carrier definition, without variants.
    fn carrier(&self) -> EntityMaterialDefinition {
        EntityMaterialDefinition {
            states: self.states.iter().copied().collect(),
            defines: self.defines.iter().cloned().collect(),
            samplers: self.samplers.iter().flatten().copied().collect(),
            vertex_fields: self.vertex_fields.as_deref().map(Box::from),
            ..self.definition.clone()
        }
    }

    /// Applies one declaration's fields with this material's version rules.
    fn apply(&mut self, fields: &Map<String, Value>) {
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
            && !self.defines.contains("ALPHA_TO_COVERAGE")
        {
            Arc::make_mut(&mut self.defines).insert("ALPHA_TO_COVERAGE".into());
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
            self.vertex_fields = Some(
                values
                    .iter()
                    .filter_map(|value| value.get("field")?.as_str())
                    .filter_map(EntityMaterialVertexField::from_name)
                    .collect(),
            );
        }
        self.apply_samplers(fields, modern);
        // Nesting, and so this recursion and the table's interning, is bounded only by the
        // JSON parser's recursion limit; parsing must keep that limit enabled.
        self.apply_variants(fields, modern);
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
    fn apply_variants(&mut self, fields: &Map<String, Value>, modern: bool) {
        let plain = fields.get("variants").and_then(Value::as_array);
        if modern && plain.is_some() {
            self.variants = Arc::default();
        }
        self.add_variants(fields, plain);
        if modern && plain.is_none() {
            let removed = fields.get("-variants").and_then(Value::as_array);
            for name in removed.into_iter().flatten().filter_map(Value::as_str) {
                if self
                    .variants
                    .iter()
                    .any(|(existing, _)| existing.as_ref() == name)
                {
                    Arc::make_mut(&mut self.variants)
                        .retain(|(existing, _)| existing.as_ref() != name);
                }
            }
        }
    }

    /// Clones and applies each entry of the plain list, or of `+variants` when it is absent,
    /// replacing an existing variant of the same name.
    fn add_variants(&mut self, fields: &Map<String, Value>, plain: Option<&Vec<Value>>) {
        let entries = plain.or_else(|| fields.get("+variants").and_then(Value::as_array));
        for entry in entries.into_iter().flatten().filter_map(Value::as_object) {
            for (name, value) in entry {
                let Some(variant_fields) = value.as_object() else {
                    continue;
                };
                if name.is_empty() || name.len() > MAX_ENTITY_MATERIAL_NAME_BYTES {
                    continue;
                }
                let mut variant = Self {
                    variants: Arc::default(),
                    ..self.clone()
                };
                if let Some(version) = text(variant_fields, "version") {
                    variant.version = u8::from(!version.starts_with('0'));
                }
                variant.apply(variant_fields);
                let variants = Arc::make_mut(&mut self.variants);
                variants.retain(|(existing, _)| existing.as_ref() != name);
                variants.push((name.as_str().into(), variant));
            }
        }
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

/// Applies `key`, `+key` and `-key` lists with the version's replace and add rules, copying the
/// shared set only when a list changes it.
fn names<T: Ord + Clone>(
    fields: &Map<String, Value>,
    key: &str,
    modern: bool,
    set: &mut Arc<BTreeSet<T>>,
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
            *set = Arc::new(values.into_iter().collect());
        } else if values.iter().any(|value| !set.contains(value)) {
            Arc::make_mut(set).extend(values);
        }
        return;
    }
    let added = list(&format!("+{key}")).unwrap_or_default();
    if added.iter().any(|value| !set.contains(value)) {
        Arc::make_mut(set).extend(added);
    }
    if modern {
        let removed = list(&format!("-{key}")).unwrap_or_default();
        if removed.iter().any(|value| set.contains(value)) {
            let set = Arc::make_mut(set);
            for value in &removed {
                set.remove(value);
            }
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

#[path = "materials/extension.rs"]
pub(crate) mod extension;
