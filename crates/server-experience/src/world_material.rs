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
}

impl WorldMaterialSpec {
    /// Whether `other` declares the same shader, textures, depth and blend, so only its
    /// parameters differ and it needs no new validation or pipeline.
    pub fn same_program(&self, other: &Self) -> bool {
        self.source == other.source
            && self.textures == other.textures
            && self.depth == other.depth
            && self.blend == other.blend
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

/// A bundle's retained world materials and bindings, by guest-chosen id, indexed so every
/// change costs a lookup rather than a scan.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldMaterials {
    materials: BTreeMap<u32, WorldMaterialSpec>,
    bindings: BTreeMap<u32, WorldBinding>,
    /// The binding id of each bound target.
    targets: BTreeMap<WorldTarget, u32>,
    /// How many bindings name each bound material.
    uses: BTreeMap<u32, usize>,
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

    /// Replaces or removes material `id`, returning the material it replaced; removal is
    /// refused while a binding names it.
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
        match spec {
            Some(spec) => {
                let charge = self.charge - held + material_charge(spec);
                ensure!(charge <= memory_bytes, MEMORY_REFUSAL);
                self.charge = charge;
                Ok(self.materials.insert(id, spec.clone()))
            }
            None => {
                ensure!(!self.uses.contains_key(&id), "world material in use");
                self.charge -= held;
                Ok(self.materials.remove(&id))
            }
        }
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

    /// Bytes the host holds of these declarations: each material's source, texture paths and
    /// parameters and each binding's identifier, plus [`WORLD_DECLARATION_OVERHEAD_BYTES`] each.
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
}

impl WorldMaterials {
    /// Restores what `undo` records. Undoing changes newest first returns to a state the
    /// declarations were in before, so it cannot be refused.
    pub fn undo(&mut self, undo: WorldUndo) {
        let restored = match undo {
            WorldUndo::Material(id, previous) => self.set_material(id, previous.as_ref()).map(drop),
            WorldUndo::Binding(id, previous) => self.set_binding(id, previous.as_ref()).map(drop),
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

/// Bytes the host holds of one binding.
fn binding_charge(binding: &WorldBinding) -> u64 {
    binding.target.identifier().len() as u64 + WORLD_DECLARATION_OVERHEAD_BYTES
}

#[cfg(test)]
mod tests;
