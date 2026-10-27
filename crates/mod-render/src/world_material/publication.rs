//! The world materials a session draws: admitted shaders with their decoded textures and
//! parameters, and the entity and block identifiers bound to them.

use super::{WorldMaterial, compute::WorldCompute};
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

/// The farthest, in blocks, any material's drawing may reach past its target's own bounds
/// within the contract's ranges ([`Effects::reach_blocks`]); terrain sections holding bound
/// blocks are culled with bounds this much larger. The client checks it against the ranges.
pub const MAX_REACH_BLOCKS: f32 = 4.25;

/// A bloom of a material's emission.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bloom {
    pub strength: f32,
    pub radius_pixels: f32,
}

/// Pixel particles around each bound target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particles {
    pub count: u32,
    pub radius_blocks: f32,
}

/// What a published material draws beyond its `shade` pass; its material's
/// [`super::Features`] provide the functions each part calls.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Effects {
    /// The displacement limit in blocks on each axis, by which culling bounds grow.
    pub displacement_blocks: Option<f32>,
    /// Whether `shade` reads the view's opaque colour and depth.
    pub scene: bool,
    /// The outline width in screen pixels.
    pub outline_pixels: Option<f32>,
    pub bloom: Option<Bloom>,
    pub particles: Option<Particles>,
    /// The index in [`WorldMaterialSet::computes`] of the cells its functions read.
    pub cells: Option<u16>,
}

impl Effects {
    /// The effects rows of the material's frame uniform: the displacement limit and the
    /// particle radius in blocks and the particle count; then the outline width in pixels and
    /// the bloom strength and radius in pixels.
    pub fn rows(&self) -> [[f32; 4]; 2] {
        [
            [
                self.displacement_blocks.unwrap_or(0.0),
                self.particles
                    .map_or(0.0, |particles| particles.radius_blocks),
                self.particles
                    .map_or(0.0, |particles| particles.count as f32),
                0.0,
            ],
            [
                self.outline_pixels.unwrap_or(0.0),
                self.bloom.map_or(0.0, |bloom| bloom.strength),
                self.bloom.map_or(0.0, |bloom| bloom.radius_pixels),
                0.0,
            ],
        ]
    }

    /// How far, in blocks, what the material draws may reach past its target's own bounds;
    /// outlines and bloom are drawn on screen around what is already seen.
    pub fn reach_blocks(&self) -> f32 {
        let displacement = self.displacement_blocks.unwrap_or(0.0);
        let particles = self
            .particles
            .map_or(0.0, |particles| particles.radius_blocks + 0.25);
        displacement.max(particles)
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
    pub depth: Depth,
    pub blend: Blend,
    pub effects: Effects,
}

/// One compute program ready to run: `cells` cells it updates each frame from the previous
/// frame's, which materials read.
#[derive(Clone, Debug, PartialEq)]
pub struct PublishedCompute {
    /// Unique for the session: a new key means new cells and a new pipeline.
    pub key: u64,
    pub compute: Arc<WorldCompute>,
    pub cells: u32,
    pub params: [f32; 16],
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
    /// The compute programs materials read cells from, in the order the host runs them.
    pub computes: Vec<PublishedCompute>,
}

impl WorldMaterialSet {
    /// A set drawing nothing, at `revision`.
    pub fn empty(revision: u64) -> Self {
        Self {
            revision,
            ..Self::default()
        }
    }

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

/// Each drawn material's parameters by key. They change without republishing the set, so a
/// parameter update compiles, uploads and remeshes nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldMaterialParams {
    /// Changes whenever any parameter changes.
    pub revision: u64,
    pub values: BTreeMap<u64, [f32; 16]>,
}
