//! Programmable world materials a client part declares for its own namespace's entities and
//! blocks: the retained declarations, their bounds and the bindings that name them.
//!
//! Shader source is validated by the host that compiles it; this module bounds what the
//! declarations may name and charges what the host holds of them to the bundle's signed memory
//! limit. No count bounds a bundle: its memory and GPU charges do.

use crate::manifest::{Permission, TEXTURE_DIR, in_namespace};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Bytes the host charges for each retained declaration on top of its own text, for its keys
/// and bookkeeping.
pub const WORLD_DECLARATION_OVERHEAD_BYTES: u64 = 64;
/// Textures one material may sample.
pub const MAX_WORLD_MATERIAL_TEXTURES: usize = 4;
/// Numbers one material passes to `param`.
pub const MAX_WORLD_MATERIAL_PARAMS: usize = 16;
/// Provisional: the largest displacement a material may declare, in blocks on each axis.
pub const MAX_DISPLACEMENT_BLOCKS: f32 = 2.0;
/// Provisional: the widest outline a material may draw, in screen pixels.
pub const MAX_OUTLINE_PIXELS: f32 = 8.0;
/// Provisional: the strongest and widest bloom a material may ask for: a multiple of its
/// emission and a blur radius in screen pixels.
pub const MAX_BLOOM_STRENGTH: f32 = 4.0;
pub const MAX_BLOOM_RADIUS_PIXELS: f32 = 16.0;
/// Provisional: the most particles one material draws around each bound target, and how far
/// from the target they may go, in blocks.
pub const MAX_PARTICLES: u32 = 4096;
pub const MAX_PARTICLE_RADIUS_BLOCKS: f32 = 4.0;
/// Provisional: the most cells one compute declaration updates, once per frame each.
pub const MAX_COMPUTE_CELLS: u32 = 65_536;
/// Provisional: the most cell updates one bundle's compute declarations may make per frame.
pub const MAX_BUNDLE_CELLS_PER_FRAME: u32 = 262_144;

/// The depth test a material draws with; the host owns the comparison and writes.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WorldDepth {
    Test,
    /// Draws through other geometry; needs `render_through_walls` as well.
    Always,
}

/// The host-defined blend class a material draws in, which also selects its pass.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WorldBlend {
    Opaque,
    /// Opaque with coverage below one half discarded.
    Cutout,
    Premultiplied,
    Additive,
}

/// One guest material: a WGSL `shade` function and the host-declared resources it reads.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorldMaterialSpec {
    pub source: String,
    /// Bundle PNG paths under `textures/`, sampled by slot.
    pub textures: Vec<String>,
    pub params: Vec<f32>,
    pub depth: WorldDepth,
    pub blend: WorldBlend,
    /// What the material does beyond `shade`, each part under its own permission.
    #[serde(default, skip_serializing_if = "WorldEffects::is_none")]
    pub effects: WorldEffects,
}

impl WorldMaterialSpec {
    /// Whether `other` declares the same shader, textures, depth, blend and effects, so only
    /// its parameters differ and it needs no new validation or pipeline.
    pub fn same_program(&self, other: &Self) -> bool {
        self.source == other.source
            && self.textures == other.textures
            && self.depth == other.depth
            && self.blend == other.blend
            && self.effects == other.effects
    }
}

/// An outline drawn around what can be seen of each bound target (`render_world_material_passes`),
/// `width_pixels` wide on screen, in the colour the material's `outline` function gives.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorldOutline {
    pub width_pixels: f32,
}

/// A bloom of the material's emission (`render_world_material_passes`): the host blurs what
/// the bound targets emit and adds it over the frame.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorldBloom {
    pub strength: f32,
    pub radius_pixels: f32,
}

/// Pixel particles around each bound target (`render_world_material_passes`): `count` squares
/// the material's `particle` function places within `radius_blocks` of the target and colours.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorldParticles {
    pub count: u32,
    pub radius_blocks: f32,
}

/// What a material does beyond `shade`. Each part needs a permission besides
/// `render_world_material`: displacement `render_world_material_vertex`, reading the scene
/// `render_world_material_scene`, and the extra passes and compute cells
/// `render_world_material_passes`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorldEffects {
    /// Moves the drawn geometry by the material's `displace` function, clamped on each axis
    /// to this many blocks; culling bounds grow by it, hit boxes and positions never move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub displacement_blocks: Option<f32>,
    /// Lets `shade` read the view's already-drawn opaque colour and depth; blended classes
    /// only.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub scene: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<WorldOutline>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bloom: Option<WorldBloom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub particles: Option<WorldParticles>,
    /// The compute declaration whose cells the material's functions read with `cell`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cells: Option<u32>,
}

impl WorldEffects {
    /// Whether the material does nothing beyond `shade`.
    pub fn is_none(&self) -> bool {
        *self == Self::default()
    }

    /// The permissions these effects need besides `render_world_material`, each once.
    pub fn permissions(&self) -> std::collections::BTreeSet<Permission> {
        let mut needed = std::collections::BTreeSet::new();
        if self.displacement_blocks.is_some() {
            needed.insert(Permission::RenderWorldMaterialVertex);
        }
        if self.scene {
            needed.insert(Permission::RenderWorldMaterialScene);
        }
        if self.outline.is_some()
            || self.bloom.is_some()
            || self.particles.is_some()
            || self.cells.is_some()
        {
            needed.insert(Permission::RenderWorldMaterialPasses);
        }
        needed
    }

    /// These effects without the parts `withdrawn` permissions allow.
    pub fn without(self, withdrawn: &std::collections::BTreeSet<Permission>) -> Self {
        let keep = |permission| !withdrawn.contains(&permission);
        let passes = keep(Permission::RenderWorldMaterialPasses);
        Self {
            displacement_blocks: self
                .displacement_blocks
                .filter(|_| keep(Permission::RenderWorldMaterialVertex)),
            scene: self.scene && keep(Permission::RenderWorldMaterialScene),
            outline: self.outline.filter(|_| passes),
            bloom: self.bloom.filter(|_| passes),
            particles: self.particles.filter(|_| passes),
            cells: self.cells.filter(|_| passes),
        }
    }
}

/// A compute declaration (`render_world_material_passes`): `cells` four-number cells the host
/// owns for the bundle, each updated once per frame by the guest's `step` function from the
/// previous frame's cells. Materials read them with `cell`; nothing reads them back to the
/// guest, and they never see host resources.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorldComputeSpec {
    pub source: String,
    pub cells: u32,
    pub params: Vec<f32>,
}

impl WorldComputeSpec {
    /// Whether `other` declares the same program and cell count, so only its parameters
    /// differ and its cells and pipeline are kept.
    pub fn same_program(&self, other: &Self) -> bool {
        self.source == other.source && self.cells == other.cells
    }

    /// GPU bytes of its cells: two generations of four 32-bit numbers each.
    pub fn cell_bytes(&self) -> u64 {
        u64::from(self.cells) * 2 * 16
    }
}

/// What a binding restyles: every bone of an entity type, or every state of a block.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WorldTarget {
    Entity(String),
    Block(String),
}

impl WorldTarget {
    pub fn identifier(&self) -> &str {
        match self {
            WorldTarget::Entity(identifier) | WorldTarget::Block(identifier) => identifier,
        }
    }
}

/// Draws `target` with the bundle's material `material`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorldBinding {
    pub target: WorldTarget,
    pub material: u32,
}

/// What a granted guest may declare, from its capabilities.
pub struct WorldAuthority<'a> {
    pub permissions: &'a std::collections::BTreeSet<Permission>,
    pub namespace: Option<&'a str>,
    pub assets: &'a std::collections::BTreeSet<String>,
}

impl WorldAuthority<'_> {
    /// Bounds one material declaration: granted, host-declared textures the bundle signs, finite
    /// bounded parameters, and through-wall depth only with that permission.
    pub fn validate_material(&self, spec: &WorldMaterialSpec) -> Result<()> {
        self.granted()?;
        ensure!(!spec.source.is_empty(), "empty world material source");
        ensure!(
            spec.textures.len() <= MAX_WORLD_MATERIAL_TEXTURES,
            "too many world material textures"
        );
        for texture in &spec.textures {
            ensure!(
                texture.starts_with(TEXTURE_DIR)
                    && texture.ends_with(".png")
                    && self.assets.contains(texture),
                "undeclared world material texture"
            );
        }
        ensure!(
            spec.params.len() <= MAX_WORLD_MATERIAL_PARAMS
                && spec.params.iter().all(|value| value.is_finite()),
            "invalid world material parameters"
        );
        ensure!(
            spec.depth == WorldDepth::Test
                || self.permissions.contains(&Permission::RenderThroughWalls),
            "through-wall depth denied"
        );
        self.validate_effects(spec)
    }

    /// Bounds a material's effects: each part's permission is held and its numbers lie in the
    /// host's ranges, and scene reads come only from blended classes, which draw after the
    /// opaque pass they read.
    pub fn validate_effects(&self, spec: &WorldMaterialSpec) -> Result<()> {
        let effects = &spec.effects;
        for permission in effects.permissions() {
            ensure!(
                self.permissions.contains(&permission),
                "world material effect denied"
            );
        }
        let within =
            |value: f32, low: f32, high: f32| value.is_finite() && (low..=high).contains(&value);
        if let Some(blocks) = effects.displacement_blocks {
            ensure!(
                within(blocks, 0.0, MAX_DISPLACEMENT_BLOCKS),
                "invalid world material displacement"
            );
        }
        ensure!(
            !effects.scene
                || matches!(spec.blend, WorldBlend::Premultiplied | WorldBlend::Additive),
            "only blended world materials read the scene"
        );
        if let Some(outline) = effects.outline {
            ensure!(
                within(outline.width_pixels, 0.0, MAX_OUTLINE_PIXELS),
                "invalid world material outline"
            );
        }
        if let Some(bloom) = effects.bloom {
            ensure!(
                within(bloom.strength, 0.0, MAX_BLOOM_STRENGTH)
                    && within(bloom.radius_pixels, 0.0, MAX_BLOOM_RADIUS_PIXELS),
                "invalid world material bloom"
            );
        }
        if let Some(particles) = effects.particles {
            ensure!(
                particles.count <= MAX_PARTICLES
                    && within(particles.radius_blocks, 0.0, MAX_PARTICLE_RADIUS_BLOCKS),
                "invalid world material particles"
            );
        }
        Ok(())
    }

    /// Bounds one compute declaration: granted, a cell count within the host's range and
    /// finite bounded parameters.
    pub fn validate_compute(&self, spec: &WorldComputeSpec) -> Result<()> {
        self.granted()?;
        ensure!(
            self.permissions
                .contains(&Permission::RenderWorldMaterialPasses),
            "world material compute denied"
        );
        ensure!(!spec.source.is_empty(), "empty world compute source");
        ensure!(
            (1..=MAX_COMPUTE_CELLS).contains(&spec.cells),
            "invalid world compute cell count"
        );
        ensure!(
            spec.params.len() <= MAX_WORLD_MATERIAL_PARAMS
                && spec.params.iter().all(|value| value.is_finite()),
            "invalid world compute parameters"
        );
        Ok(())
    }

    /// A binding may name only an identifier in the bundle's signed namespace, unless the
    /// bundle holds `render_world_material_override`, which lets it name any identifier,
    /// vanilla included.
    pub fn validate_binding(&self, binding: &WorldBinding) -> Result<()> {
        self.granted()?;
        let identifier = binding.target.identifier();
        if self
            .permissions
            .contains(&Permission::RenderWorldMaterialOverride)
        {
            ensure!(
                crate::manifest::identifier(identifier) && identifier.contains(':'),
                "invalid binding identifier"
            );
            return Ok(());
        }
        let namespace = self
            .namespace
            .ok_or_else(|| anyhow::anyhow!("no signed namespace"))?;
        ensure!(
            in_namespace(identifier, namespace),
            "binding outside the bundle's namespace"
        );
        Ok(())
    }

    fn granted(&self) -> Result<()> {
        ensure!(
            self.permissions.contains(&Permission::RenderWorldMaterial),
            "capability denied"
        );
        Ok(())
    }
}

/// A bundle's retained world materials, bindings and compute declarations, by guest-chosen
/// id, indexed so every change costs a lookup rather than a scan.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldMaterials {
    materials: BTreeMap<u32, WorldMaterialSpec>,
    bindings: BTreeMap<u32, WorldBinding>,
    computes: BTreeMap<u32, WorldComputeSpec>,
    /// The binding id of each bound target.
    targets: BTreeMap<WorldTarget, u32>,
    /// How many bindings name each bound material.
    uses: BTreeMap<u32, usize>,
    /// How many materials read each compute declaration's cells.
    readers: BTreeMap<u32, usize>,
    /// The cells every compute declaration updates a frame, together.
    cells: u64,
    /// [`Self::charge`], kept as declarations change.
    charge: u64,
}

impl WorldMaterials {
    /// The held materials by id.
    pub fn materials(&self) -> &BTreeMap<u32, WorldMaterialSpec> {
        &self.materials
    }

    /// The held bindings by id.
    pub fn bindings(&self) -> &BTreeMap<u32, WorldBinding> {
        &self.bindings
    }

    /// The held compute declarations by id.
    pub fn computes(&self) -> &BTreeMap<u32, WorldComputeSpec> {
        &self.computes
    }

    /// Replaces or removes material `id`, returning the material it replaced; removal is
    /// refused while a binding names it, and a material reading cells must name a held
    /// compute declaration.
    pub fn set_material(
        &mut self,
        id: u32,
        spec: Option<&WorldMaterialSpec>,
    ) -> Result<Option<WorldMaterialSpec>> {
        self.set_material_within(id, spec, u64::MAX)
    }

    /// [`Self::set_material`], refused with nothing changed when the declarations would then
    /// exceed `memory_bytes`.
    pub fn set_material_within(
        &mut self,
        id: u32,
        spec: Option<&WorldMaterialSpec>,
        memory_bytes: u64,
    ) -> Result<Option<WorldMaterialSpec>> {
        let held = self.materials.get(&id).map_or(0, material_charge);
        let previous = match spec {
            Some(spec) => {
                ensure!(
                    spec.effects
                        .cells
                        .is_none_or(|compute| self.computes.contains_key(&compute)),
                    "unknown world compute"
                );
                let charge = self.charge - held + material_charge(spec);
                ensure!(charge <= memory_bytes, MEMORY_REFUSAL);
                self.charge = charge;
                if let Some(compute) = spec.effects.cells {
                    *self.readers.entry(compute).or_default() += 1;
                }
                self.materials.insert(id, spec.clone())
            }
            None => {
                ensure!(!self.uses.contains_key(&id), "world material in use");
                self.charge -= held;
                self.materials.remove(&id)
            }
        };
        if let Some(compute) = previous.as_ref().and_then(|spec| spec.effects.cells)
            && let Some(readers) = self.readers.get_mut(&compute)
        {
            *readers -= 1;
            if *readers == 0 {
                self.readers.remove(&compute);
            }
        }
        Ok(previous)
    }

    /// Replaces or removes binding `id`, returning the binding it replaced; a binding must
    /// name a held material, and one target takes one binding.
    pub fn set_binding(
        &mut self,
        id: u32,
        binding: Option<&WorldBinding>,
    ) -> Result<Option<WorldBinding>> {
        self.set_binding_within(id, binding, u64::MAX)
    }

    /// [`Self::set_binding`], refused with nothing changed when the declarations would then
    /// exceed `memory_bytes`.
    pub fn set_binding_within(
        &mut self,
        id: u32,
        binding: Option<&WorldBinding>,
        memory_bytes: u64,
    ) -> Result<Option<WorldBinding>> {
        let held = self.bindings.get(&id).map_or(0, binding_charge);
        if let Some(binding) = binding {
            ensure!(
                self.materials.contains_key(&binding.material),
                "unknown world material"
            );
            ensure!(
                self.targets
                    .get(&binding.target)
                    .is_none_or(|other| *other == id),
                "target already bound"
            );
            let charge = self.charge - held + binding_charge(binding);
            ensure!(charge <= memory_bytes, MEMORY_REFUSAL);
            self.charge = charge;
        } else {
            self.charge -= held;
        }
        let previous = match binding {
            Some(binding) => self.bindings.insert(id, binding.clone()),
            None => self.bindings.remove(&id),
        };
        if let Some(previous) = &previous {
            self.targets.remove(&previous.target);
            if let Some(uses) = self.uses.get_mut(&previous.material) {
                *uses -= 1;
                if *uses == 0 {
                    self.uses.remove(&previous.material);
                }
            }
        }
        if let Some(binding) = binding {
            self.targets.insert(binding.target.clone(), id);
            *self.uses.entry(binding.material).or_default() += 1;
        }
        Ok(previous)
    }

    /// Replaces or removes compute declaration `id`, returning the declaration it replaced;
    /// removal is refused while a material reads its cells, and the bundle's computes together
    /// update at most [`MAX_BUNDLE_CELLS_PER_FRAME`] cells a frame.
    pub fn set_compute(
        &mut self,
        id: u32,
        spec: Option<&WorldComputeSpec>,
    ) -> Result<Option<WorldComputeSpec>> {
        self.set_compute_within(id, spec, u64::MAX)
    }

    /// [`Self::set_compute`], refused with nothing changed when the declarations would then
    /// exceed `memory_bytes`.
    pub fn set_compute_within(
        &mut self,
        id: u32,
        spec: Option<&WorldComputeSpec>,
        memory_bytes: u64,
    ) -> Result<Option<WorldComputeSpec>> {
        let held = self.computes.get(&id);
        let (held_bytes, held_cells) =
            held.map_or((0, 0), |held| (compute_charge(held), u64::from(held.cells)));
        match spec {
            Some(spec) => {
                let cells = self.cells - held_cells + u64::from(spec.cells);
                ensure!(
                    cells <= u64::from(MAX_BUNDLE_CELLS_PER_FRAME),
                    "world computes exceed the bundle's cells per frame"
                );
                let charge = self.charge - held_bytes + compute_charge(spec);
                ensure!(charge <= memory_bytes, MEMORY_REFUSAL);
                self.charge = charge;
                self.cells = cells;
                Ok(self.computes.insert(id, spec.clone()))
            }
            None => {
                ensure!(!self.readers.contains_key(&id), "world compute in use");
                self.charge -= held_bytes;
                self.cells -= held_cells;
                Ok(self.computes.remove(&id))
            }
        }
    }

    /// Bytes the host holds of these declarations: each material's and compute's source,
    /// texture paths and parameters and each binding's identifier, plus
    /// [`WORLD_DECLARATION_OVERHEAD_BYTES`] each.
    pub fn charge(&self) -> u64 {
        self.charge
    }

    /// Whether the held declarations fit the bundle's signed memory limit.
    pub fn check(&self, memory_bytes: u64) -> Result<()> {
        ensure!(self.charge <= memory_bytes, MEMORY_REFUSAL);
        Ok(())
    }
}

/// The previous value of one changed declaration, which restores it.
#[derive(Debug)]
pub enum WorldUndo {
    Material(u32, Option<WorldMaterialSpec>),
    Binding(u32, Option<WorldBinding>),
    Compute(u32, Option<WorldComputeSpec>),
}

impl WorldMaterials {
    /// Restores what `undo` records. Undoing changes newest first returns to a state the
    /// declarations were in before, so it cannot be refused.
    pub fn undo(&mut self, undo: WorldUndo) {
        let restored = match undo {
            WorldUndo::Material(id, previous) => self.set_material(id, previous.as_ref()).map(drop),
            WorldUndo::Binding(id, previous) => self.set_binding(id, previous.as_ref()).map(drop),
            WorldUndo::Compute(id, previous) => self.set_compute(id, previous.as_ref()).map(drop),
        };
        debug_assert!(restored.is_ok(), "an undo restores a held state");
    }
}

/// Why a declaration that would exceed the bundle's memory limit is refused.
const MEMORY_REFUSAL: &str = "world materials exceed the bundle's memory limit";

/// Bytes the host holds of one material declaration.
fn material_charge(spec: &WorldMaterialSpec) -> u64 {
    spec.source.len() as u64
        + spec
            .textures
            .iter()
            .map(|path| path.len() as u64)
            .sum::<u64>()
        + spec.params.len() as u64 * 4
        + WORLD_DECLARATION_OVERHEAD_BYTES
}

/// Bytes the host holds of one compute declaration.
fn compute_charge(spec: &WorldComputeSpec) -> u64 {
    spec.source.len() as u64 + spec.params.len() as u64 * 4 + WORLD_DECLARATION_OVERHEAD_BYTES
}

/// Bytes the host holds of one binding.
fn binding_charge(binding: &WorldBinding) -> u64 {
    binding.target.identifier().len() as u64 + WORLD_DECLARATION_OVERHEAD_BYTES
}

#[cfg(test)]
mod tests;
