//! Programmable world materials a client part declares for its own namespace's entities and
//! blocks: the retained declarations, their bounds and the bindings that name them.
//!
//! Shader source is validated by the host that compiles it; this module bounds only what the
//! declarations may name and how many a bundle may hold.

use crate::manifest::{Permission, TEXTURE_DIR, in_namespace};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Provisional: materials one bundle may hold at once.
pub const MAX_WORLD_MATERIALS: usize = 8;
/// Provisional: bindings one bundle may hold at once.
pub const MAX_WORLD_BINDINGS: usize = 64;
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

    /// A binding may name only an identifier in the bundle's signed namespace.
    pub fn validate_binding(&self, binding: &WorldBinding) -> Result<()> {
        self.granted()?;
        let namespace = self
            .namespace
            .ok_or_else(|| anyhow::anyhow!("no signed namespace"))?;
        ensure!(
            in_namespace(binding.target.identifier(), namespace),
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

/// A bundle's retained world materials and bindings, by guest-chosen id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldMaterials {
    pub materials: BTreeMap<u32, WorldMaterialSpec>,
    pub bindings: BTreeMap<u32, WorldBinding>,
}

impl WorldMaterials {
    /// Replaces or removes material `id`; removal is refused while a binding names it.
    pub fn set_material(&mut self, id: u32, spec: Option<&WorldMaterialSpec>) -> Result<()> {
        match spec {
            Some(spec) => {
                self.materials.insert(id, spec.clone());
            }
            None => {
                ensure!(
                    !self.bindings.values().any(|binding| binding.material == id),
                    "world material in use"
                );
                self.materials.remove(&id);
            }
        }
        Ok(())
    }

    /// Replaces or removes binding `id`; a binding must name a held material, and one target
    /// takes one binding.
    pub fn set_binding(&mut self, id: u32, binding: Option<&WorldBinding>) -> Result<()> {
        match binding {
            Some(binding) => {
                ensure!(
                    self.materials.contains_key(&binding.material),
                    "unknown world material"
                );
                ensure!(
                    !self
                        .bindings
                        .iter()
                        .any(|(other, held)| *other != id && held.target == binding.target),
                    "target already bound"
                );
                self.bindings.insert(id, binding.clone());
            }
            None => {
                self.bindings.remove(&id);
            }
        }
        Ok(())
    }

    /// Whether the held declarations fit the per-bundle bounds.
    pub fn check(&self) -> Result<()> {
        ensure!(
            self.materials.len() <= MAX_WORLD_MATERIALS,
            "world material budget exceeded"
        );
        ensure!(
            self.bindings.len() <= MAX_WORLD_BINDINGS,
            "world binding budget exceeded"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests;
