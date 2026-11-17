//! Collision boxes of 1.7.10 blocks.
//!
//! The game keeps each block class's bounds as `f64` fields that are mostly assigned from `f32`
//! values, and builds a collision box by adding the block position to them in `f64`. Some classes
//! instead build the box from `f32` sums of the position. Both paths are reproduced operation by
//! operation so the boxes match the game bit for bit.
//!
//! Bounds are shared by every block of a class and several classes read them without refreshing
//! them first. Where that makes the game's answer depend on which block of the class last had its
//! bounds refreshed (by rendering, targeting or an earlier collision query), this model answers as
//! if the queried block itself was the last one; see [`partial::anvil`], [`partial::chest`] and
//! [`partial::ladder`].

mod compound;
mod connecting;
mod hinged;
mod partial;
mod stairs;

use super::Block;
use super::facts::entry_of;
use crate::math::Aabb;
use crate::world::World;

/// How a block's class builds its collision boxes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    /// No collision box.
    None,
    /// The unit cube.
    FullCube,
    Bed,
    PistonBase,
    PistonHead,
    /// A block being moved by a piston. Its collision box comes from the piston tile entity's
    /// stored block, direction and progress, which this model does not carry yet, so it has no
    /// collision here.
    MovingPiston,
    Slab,
    Stairs,
    Chest,
    Farmland,
    Door,
    Ladder,
    SnowLayer,
    Cactus,
    Fence,
    SoulSand,
    Cake,
    /// Repeaters and comparators.
    Diode,
    Trapdoor,
    Pane,
    FenceGate,
    LilyPad,
    EnchantingTable,
    BrewingStand,
    Cauldron,
    EndPortalFrame,
    DragonEgg,
    Cocoa,
    EnderChest,
    Wall,
    FlowerPot,
    Skull,
    Anvil,
    DaylightDetector,
    Hopper,
    Carpet,
}

/// Block-local bounds as the game stores them: `f32` values widened to `f64`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
    max_z: f64,
}

impl Bounds {
    /// The unit cube every block class starts with.
    const FULL: Self = Self::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);

    /// Bounds from the `f32` corner values the game assigns, minimum corner first.
    const fn new(min_x: f32, min_y: f32, min_z: f32, max_x: f32, max_y: f32, max_z: f32) -> Self {
        Self {
            min_x: min_x as f64,
            min_y: min_y as f64,
            min_z: min_z as f64,
            max_x: max_x as f64,
            max_y: max_y as f64,
            max_z: max_z as f64,
        }
    }
}

/// Collects the collision boxes of the block at one position, keeping those that intersect the
/// query mask.
struct Collector<'a> {
    x: i32,
    y: i32,
    z: i32,
    mask: &'a Aabb,
    out: &'a mut Vec<Aabb>,
}

impl Collector<'_> {
    /// Offers block-local bounds: the box is the block position plus the bounds, summed in `f64`.
    fn local(&mut self, bounds: Bounds) {
        let (x, y, z) = (f64::from(self.x), f64::from(self.y), f64::from(self.z));
        self.world(Aabb::new(
            x + bounds.min_x,
            y + bounds.min_y,
            z + bounds.min_z,
            x + bounds.max_x,
            y + bounds.max_y,
            z + bounds.max_z,
        ));
    }

    /// Offers a box already in world coordinates.
    fn world(&mut self, aabb: Aabb) {
        if self.mask.intersects(&aabb) {
            self.out.push(aabb);
        }
    }
}

/// The 4-bit metadata of a block as the `int` the game's shape code works on.
fn meta_of(block: Block) -> i32 {
    i32::from(block.meta & 0x0F)
}

/// Appends the collision boxes of the block at `(x, y, z)` that intersect `mask`, in the game's
/// order, as the block's class adds them for a player.
///
/// Unknown ids are air and add nothing. Moving-piston blocks add nothing because their collision
/// needs tile-entity state this model does not carry yet.
pub fn collision_boxes(
    world: &impl World,
    x: i32,
    y: i32,
    z: i32,
    mask: &Aabb,
    out: &mut Vec<Aabb>,
) {
    let block = world.block(x, y, z);
    let meta = meta_of(block);
    let mut boxes = Collector { x, y, z, mask, out };
    match entry_of(block.id).shape {
        Shape::None | Shape::MovingPiston => {}
        Shape::FullCube => boxes.local(Bounds::FULL),
        Shape::Bed => boxes.local(partial::BED),
        Shape::Diode => boxes.local(partial::DIODE),
        Shape::LilyPad => boxes.local(partial::LILY_PAD),
        Shape::EnchantingTable => boxes.local(partial::ENCHANTING_TABLE),
        Shape::DragonEgg => boxes.local(partial::DRAGON_EGG),
        Shape::EnderChest => boxes.local(partial::SINGLE_CHEST),
        Shape::FlowerPot => boxes.local(partial::FLOWER_POT),
        Shape::DaylightDetector => boxes.local(partial::DAYLIGHT_DETECTOR),
        Shape::Slab => boxes.local(partial::slab(meta)),
        Shape::Ladder => boxes.local(partial::ladder(meta)),
        Shape::Skull => boxes.local(partial::skull(meta)),
        Shape::Cocoa => boxes.local(partial::cocoa(meta)),
        Shape::Anvil => boxes.local(partial::anvil(meta)),
        Shape::Chest => boxes.local(partial::chest(world, x, y, z, block.id)),
        Shape::Farmland => boxes.world(partial::farmland(x, y, z)),
        Shape::SnowLayer => boxes.world(partial::snow_layer(x, y, z, meta)),
        Shape::Carpet => boxes.world(partial::carpet(x, y, z)),
        Shape::Cactus => boxes.world(partial::cactus(x, y, z)),
        Shape::SoulSand => boxes.world(partial::soul_sand(x, y, z)),
        Shape::Cake => boxes.world(partial::cake(x, y, z, meta)),
        Shape::Door => boxes.local(hinged::door(world, x, y, z, meta)),
        Shape::Trapdoor => boxes.local(hinged::trapdoor(meta)),
        Shape::FenceGate => {
            if let Some(gate) = hinged::fence_gate(x, y, z, meta) {
                boxes.world(gate);
            }
        }
        Shape::PistonBase => boxes.local(compound::piston_base(meta)),
        Shape::PistonHead => compound::piston_head(&mut boxes, meta),
        Shape::Cauldron => compound::basin(&mut boxes, compound::CAULDRON_FLOOR),
        Shape::Hopper => compound::basin(&mut boxes, compound::HOPPER_FLOOR),
        Shape::BrewingStand => compound::brewing_stand(&mut boxes),
        Shape::EndPortalFrame => compound::end_portal_frame(&mut boxes, meta),
        Shape::Fence => connecting::fence(&mut boxes, world, block.id),
        Shape::Wall => connecting::wall(&mut boxes, world, block.id),
        Shape::Pane => connecting::pane(&mut boxes, world, block.id),
        Shape::Stairs => stairs::stairs(&mut boxes, world, meta),
    }
}
