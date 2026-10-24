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
    /// Entity raster sources by extensionless path, for extension texture lookups.
    textures: BTreeMap<Box<str>, u32>,
    files: usize,
    definitions: BTreeMap<Box<str>, Declaration>,
    /// `+name` declarations applied in place after the named material resolves.
    modifications: BTreeMap<Box<str>, Vec<Modification>>,
    /// Declarations past the symbol bound, which stay unresolved.
    unadmitted: u32,
    /// The defines each admitted file's root material holds, by file index.
    file_defines: Vec<BTreeSet<Box<str>>>,
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
    /// Whether an absent `fragmentShader` stands for the native entity program: true for the
    /// native bases the client declares itself, whose shipped programs it cannot see, and for
    /// extension materials without a base.
    assumes_entity_program: bool,
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
    /// Interned Cinnabar glow and emissive mask pairs.
    masks: Vec<assets::EntityMaterialMasks>,
}

impl MaterialTable {
    /// The finished definition and mask tables, in index order.
    pub(super) fn into_parts(
        self,
    ) -> (
        Box<[EntityMaterialDefinition]>,
        Box<[assets::EntityMaterialMasks]>,
    ) {
        (
            self.definitions.into_boxed_slice(),
            self.masks.into_boxed_slice(),
        )
    }

    /// Textures a draw with definition `index` needs, as Bedrock validates them before drawing:
    /// `GLINT` needs a second texture, and with `MULTIPLICATIVE_TINT` or `MASKED_MULTITEXTURE`
    /// a third; either of those alone needs a second. Other materials need none here, so layers
    /// with unresolved textures keep their artwork fallback.
    pub(super) fn required_textures(&self, index: u32) -> u16 {
        let Some(definition) = self.definitions.get(index as usize) else {
            return 0;
        };
        let glint = definition.has_define("GLINT");
        let tint = definition.has_define("MULTIPLICATIVE_TINT")
            || definition.has_define("MASKED_MULTITEXTURE");
        match (glint, tint) {
            (true, true) => 3,
            (true, false) | (false, true) => 2,
            (false, false) => 0,
        }
    }

    /// Interns `masks`, reusing an equal existing entry. Masks past the carrier bound stay
    /// unresolved and are counted; their material draws without them.
    fn intern_masks(&mut self, masks: assets::EntityMaterialMasks) -> Option<u32> {
        if let Some(index) = self.masks.iter().position(|existing| *existing == masks) {
            return Some(index as u32);
        }
        if self.masks.len() >= MAX_ENTITY_MATERIAL_DEFINITIONS {
            self.unresolved += 1;
            return None;
        }
        self.masks.push(masks);
        Some(self.masks.len() as u32 - 1)
    }

    /// Definitions, variants and mask pairs that did not fit the carrier bound. Their materials
    /// draw without them, and the rest of the pack still compiles.
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
    /// Reads retained material files once for this compilation: the entity material list's
    /// files first, in list order and with its defines, then unlisted files in path order.
    pub(super) fn load(
        root: &Path,
        payloads: &SourceDocuments<'_>,
        sources: &[EntityAssetSource],
    ) -> Result<Self, AssetError> {
        let mut documents = Vec::new();
        for source in sources
            .iter()
            .filter(|source| is_material_source(&source.path))
        {
            documents.push((source.path.as_ref(), payloads.read(root, &source.path)?));
        }
        let mut states = Self::from_documents(documents);
        states.textures = sources
            .iter()
            .enumerate()
            .filter_map(|(index, source)| {
                let stem = source
                    .path
                    .strip_suffix(".png")
                    .or_else(|| source.path.strip_suffix(".tga"))?;
                Some((stem.into(), index as u32))
            })
            .collect();
        if sources
            .iter()
            .any(|source| source.path.as_ref() == extension::MATERIAL_BINDINGS_PATH)
        {
            states.extension =
                extension::parse(payloads.read(root, extension::MATERIAL_BINDINGS_PATH)?).ok();
        }
        Ok(states)
    }

    /// Admits material documents by path as the client loads them. Files the entity material
    /// list names load first, in list order, each declaration starting from a root that holds
    /// the entry's defines. Other `materials/*.material` files then load in path order with no
    /// list defines: they stand in for the vanilla lists, which name vanilla paths a pack may
    /// override but which the client does not ship.
    pub(crate) fn from_documents<'a>(
        documents: impl IntoIterator<Item = (&'a str, &'a Value)>,
    ) -> Self {
        let documents = documents.into_iter().collect::<BTreeMap<_, _>>();
        let (entries, _) = documents
            .get(list::ENTITY_MATERIAL_LIST_PATH)
            .map(|list| list::resolve_material_lists([*list]))
            .unwrap_or_default();
        let mut states = Self::default();
        let mut listed = BTreeSet::new();
        for entry in entries {
            if let Some(document) = documents.get(entry.path.as_ref()) {
                listed.insert(entry.path.clone());
                states.read_with(document, &entry.defines);
            }
        }
        for (path, document) in documents {
            if is_material_file(path) && !listed.contains(path) {
                states.read(document);
            }
        }
        states
    }

    /// Admits one file with no list defines.
    pub(super) fn read(&mut self, value: &Value) {
        self.read_with(value, &BTreeSet::new());
    }

    /// Admits one file whose root material holds `defines`. Without a `+` prefix the first
    /// declaration of a name wins.
    fn read_with(&mut self, value: &Value, defines: &BTreeSet<Box<str>>) {
        let file = self.files;
        self.files += 1;
        self.file_defines.push(defines.clone());
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
        self.file_defines.extend(upper.file_defines);
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
            .and_then(|resolved| {
                let drawn = resolved.actor_variant();
                Some((actor_shading(drawn), table.intern(drawn)?))
            });
        table.resolved.insert(name.into(), resolved);
        resolved
    }

    /// Resolves a pack's block bindings: material documents by path supply bases as
    /// [`Self::from_documents`] loads them, and bindings whose material does not resolve are
    /// dropped.
    pub(crate) fn block_bindings(
        material_documents: &[(String, Value)],
        document: &Value,
    ) -> Vec<extension::ResolvedBlockBinding> {
        let mut states = Self::from_documents(
            material_documents
                .iter()
                .map(|(path, document)| (path.as_str(), document)),
        );
        states.extension = extension::parse(document).ok();
        let mut table = MaterialTable::default();
        let Some(extension) = &states.extension else {
            return Vec::new();
        };
        extension
            .block_bindings
            .iter()
            .filter_map(|binding| {
                let (shading, index, _) =
                    states.resolve_extension(&binding.material, false, &mut table)?;
                Some(extension::ResolvedBlockBinding {
                    block: binding.block.clone(),
                    states: binding.states.clone(),
                    material_instance: binding.material_instance.clone(),
                    definition: table.definitions[index as usize].clone(),
                    shading,
                    masks: extension.materials[&binding.material].mask_paths(),
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
    /// features with the definition's table index and its interned masks, if any. `actor`
    /// selects the shading of an actor draw, which follows the actor material type.
    pub(super) fn resolve_extension(
        &self,
        name: &str,
        actor: bool,
        table: &mut MaterialTable,
    ) -> Option<(EntityRenderMaterialState, u32, Option<u32>)> {
        let material = self.extension.as_ref()?.materials.get(name)?;
        let mut resolved = match &material.base {
            Some(base) => self.resolve_with(material_name(base), &builtin, &mut table.chains)?,
            // Without a base the constructor defaults name no program; the extension draws
            // such a material with the entity program.
            None => Resolved {
                assumes_entity_program: true,
                ..Resolved::default()
            },
        };
        resolved.version = 1;
        resolved.apply(&material.fields);
        let masks = material
            .masks(|path| self.textures.get(path).copied())
            .and_then(|masks| table.intern_masks(masks));
        let drawn = resolved.actor_variant();
        let shading = if actor {
            actor_shading(drawn)
        } else {
            shading(drawn)
        };
        Some((shading, table.intern(drawn)?, masks))
    }

    /// Adds the root defines of `file`'s list entry to a material starting from that root.
    fn seed_root_defines(&self, resolved: &mut Resolved, file: usize) {
        if let Some(defines) = self
            .file_defines
            .get(file)
            .filter(|defines| !defines.is_empty())
        {
            Arc::make_mut(&mut resolved.defines).extend(defines.iter().cloned());
        }
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
                // A built-in base the pack never declares still takes its modifications. It
                // stands in for a declaration of the child's own file, so it starts from that
                // file's root.
                let resolved = base(current).and_then(|mut resolved| {
                    if let Some(file) = file {
                        self.seed_root_defines(&mut resolved, file);
                    }
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
                None => {
                    let mut resolved = base(current).unwrap_or_default();
                    self.seed_root_defines(&mut resolved, declaration.file);
                    break Some(resolved);
                }
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

/// Whether `path` is a source material admission reads: the entity material list or a
/// `materials/*.material` file.
pub(crate) fn is_material_source(path: &str) -> bool {
    path == list::ENTITY_MATERIAL_LIST_PATH || is_material_file(path)
}

/// Whether `path` is a `materials/*.material` file.
fn is_material_file(path: &str) -> bool {
    path.starts_with("materials/") && path.ends_with(".material")
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

/// The variant an actor draw selects.
const SKINNING_VARIANT: &str = "skinning";

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
        assumes_entity_program: true,
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

/// The shading features an actor draw reads: [`shading`] for the material's actor material
/// type. A material whose program has no actor type draws as plain `Actor`, whose name the
/// multitexture define no longer selects.
fn actor_shading(resolved: &Resolved) -> EntityRenderMaterialState {
    let mut state = shading(resolved);
    if actor_type::of(resolved) == actor_type::ActorMaterialType::Other {
        state.multitexture = false;
    }
    state
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

    /// What an actor draw uses: Bedrock looks up `<name>.skinning` for every client-entity
    /// material and falls back to the material itself. Actor meshes carry no vertex colour, so
    /// the `skinning_color` variant is not selected.
    fn actor_variant(&self) -> &Self {
        self.variants
            .iter()
            .find(|(name, _)| name.as_ref() == SKINNING_VARIANT)
            .map_or(self, |(_, variant)| variant)
    }
}

#[path = "materials/fields.rs"]
mod fields;
use fields::lists_are_well_formed;

#[cfg(test)]
#[path = "materials/tests.rs"]
mod tests;

#[path = "materials/extension.rs"]
pub(crate) mod extension;

#[path = "materials/list.rs"]
pub(crate) mod list;

#[path = "materials/actor_type.rs"]
mod actor_type;
