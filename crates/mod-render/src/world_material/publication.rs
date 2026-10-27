//! The world materials a session draws: admitted shaders with their decoded textures and
//! parameters, and the entity and block identifiers bound to them.

use super::WorldMaterial;
use std::{collections::BTreeMap, sync::Arc};

/// Slots a block binding can name: block quads carry one more than the slot in six bits.
/// The renderer checks this against meshing's tag width.
pub const MAX_BLOCK_SLOTS: usize = 63;
/// Slots an entity binding can name: actor material words carry one more than the slot in
/// eight bits. The renderer checks this against the actor material word.
pub const MAX_ENTITY_SLOTS: usize = 255;

/// The depth test a published material draws with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Depth {
    Test,
    Always,
}

/// The host blend class, which also selects the pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Blend {
    Opaque,
    Cutout,
    Premultiplied,
    Additive,
}

/// A declared texture: sRGB RGBA8, row-major, top row first.
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

impl Texture {
    /// GPU bytes the texture occupies once uploaded.
    pub fn bytes(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height) * 4
    }
}

/// One material ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Published {
    /// Unique for the session: a new key means a new shader or texture set, which a renderer
    /// compiles and uploads; equal keys share them.
    pub key: u64,
    pub material: Arc<WorldMaterial>,
    pub textures: Arc<[Texture]>,
    pub params: [f32; 16],
    pub depth: Depth,
    pub blend: Blend,
}

/// Every drawable world material and what binds to it. Slots index `materials`; an empty slot
/// keeps its index free so the slots after it, and the tags meshes carry for them, stay put.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldMaterialSet {
    /// Changes whenever any material or binding changes.
    pub revision: u64,
    pub materials: Vec<Option<Published>>,
    /// Entity identifier to the slot its every bone draws with.
    pub entities: BTreeMap<Box<str>, u16>,
    /// Block identifier to the slot its every state draws with.
    pub blocks: BTreeMap<Box<str>, u16>,
}

impl WorldMaterialSet {
    /// Every published material, in slot order.
    pub fn published(&self) -> impl Iterator<Item = &Published> {
        self.materials.iter().flatten()
    }

    /// The material bound to entity `identifier`, if any.
    pub fn entity(&self, identifier: &str) -> Option<(u16, &Published)> {
        let slot = *self.entities.get(identifier)?;
        Some((slot, self.materials.get(usize::from(slot))?.as_ref()?))
    }

    /// The material bound to block `identifier`, if any.
    pub fn block(&self, identifier: &str) -> Option<(u16, &Published)> {
        let slot = *self.blocks.get(identifier)?;
        Some((slot, self.materials.get(usize::from(slot))?.as_ref()?))
    }
}
