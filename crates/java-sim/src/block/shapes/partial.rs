//! Single-box shapes that depend only on the block's own position and metadata, plus the chest.

use super::Bounds;
use crate::math::Aabb;
use crate::world::World;

/// One sixteenth of a block, the unit most 1.7.10 shapes are measured in.
const SIXTEENTH: f32 = 0.0625;
/// One eighth of a block.
const EIGHTH: f32 = 0.125;

pub(super) const BED: Bounds = Bounds::new(0.0, 0.0, 0.0, 1.0, 0.5625, 1.0);
/// Repeaters and comparators keep their constructor bounds.
pub(super) const DIODE: Bounds = Bounds::new(0.0, 0.0, 0.0, 1.0, EIGHTH, 1.0);
pub(super) const LILY_PAD: Bounds = Bounds::new(0.0, 0.0, 0.0, 1.0, 0.015625, 1.0);
pub(super) const ENCHANTING_TABLE: Bounds = Bounds::new(0.0, 0.0, 0.0, 1.0, 0.75, 1.0);
pub(super) const DRAGON_EGG: Bounds = Bounds::new(0.0625, 0.0, 0.0625, 0.9375, 1.0, 0.9375);
pub(super) const DAYLIGHT_DETECTOR: Bounds = Bounds::new(0.0, 0.0, 0.0, 1.0, 0.375, 1.0);
/// A chest with no chest beside it; ender chests always use it.
pub(super) const SINGLE_CHEST: Bounds = Bounds::new(0.0625, 0.0, 0.0625, 0.9375, 0.875, 0.9375);

/// The flower pot: its height and, centred on the block, half of it to each side.
pub(super) const FLOWER_POT: Bounds = {
    let height: f32 = 0.375;
    let half = height / 2.0;
    Bounds::new(0.5 - half, 0.0, 0.5 - half, 0.5 + half, height, 0.5 + half)
};

/// A single slab: the top half when metadata bit 8 is set, otherwise the bottom half.
pub(super) fn slab(meta: i32) -> Bounds {
    if meta & 8 != 0 {
        Bounds::new(0.0, 0.5, 0.0, 1.0, 1.0, 1.0)
    } else {
        Bounds::new(0.0, 0.0, 0.0, 1.0, 0.5, 1.0)
    }
}

/// A ladder: an eighth-thick slab against the wall its metadata (2 to 5) names.
///
/// The game leaves the class bounds untouched for any other metadata, so its answer is whatever
/// ladder was refreshed last. This model returns the unit cube the class starts with.
pub(super) fn ladder(meta: i32) -> Bounds {
    let thickness = EIGHTH;
    match meta {
        2 => Bounds::new(0.0, 0.0, 1.0 - thickness, 1.0, 1.0, 1.0),
        3 => Bounds::new(0.0, 0.0, 0.0, 1.0, 1.0, thickness),
        4 => Bounds::new(1.0 - thickness, 0.0, 0.0, 1.0, 1.0, 1.0),
        5 => Bounds::new(0.0, 0.0, 0.0, thickness, 1.0, 1.0),
        _ => Bounds::FULL,
    }
}

/// A skull: on the floor, or on the wall its low three metadata bits (2 to 5) name.
pub(super) fn skull(meta: i32) -> Bounds {
    match meta & 7 {
        2 => Bounds::new(0.25, 0.25, 0.5, 0.75, 0.75, 1.0),
        3 => Bounds::new(0.25, 0.25, 0.0, 0.75, 0.75, 0.5),
        4 => Bounds::new(0.5, 0.25, 0.25, 1.0, 0.75, 0.75),
        5 => Bounds::new(0.0, 0.25, 0.25, 0.5, 0.75, 0.75),
        _ => Bounds::new(0.25, 0.0, 0.25, 0.75, 0.5, 0.75),
    }
}

/// A cocoa pod: hung from the side its low two metadata bits name and grown by its age in
/// bits 4 and 8.
pub(super) fn cocoa(meta: i32) -> Bounds {
    let direction = meta & 3;
    let age = (meta & 0xC) >> 2;
    let width = (4 + age * 2) as f32;
    let height = (5 + age * 2) as f32;
    let half_width = width / 2.0;
    let near = (8.0 - half_width) / 16.0;
    let far = (8.0 + half_width) / 16.0;
    let bottom = (12.0 - height) / 16.0;
    match direction {
        0 => Bounds::new(near, bottom, (15.0 - width) / 16.0, far, 0.75, 0.9375),
        1 => Bounds::new(0.0625, bottom, near, (1.0 + width) / 16.0, 0.75, far),
        2 => Bounds::new(near, bottom, 0.0625, far, 0.75, (1.0 + width) / 16.0),
        _ => Bounds::new((15.0 - width) / 16.0, bottom, near, 0.9375, 0.75, far),
    }
}

/// An anvil: three quarters wide across the axis its low two metadata bits name.
///
/// The game's collision reads the anvil class bounds without refreshing them, so every anvil
/// collides with the shape of the anvil that was rendered or targeted last (the unit cube before
/// any was). This model uses the queried anvil's own orientation.
pub(super) fn anvil(meta: i32) -> Bounds {
    let direction = meta & 3;
    if direction == 3 || direction == 1 {
        Bounds::new(0.0, 0.0, 0.125, 1.0, 1.0, 0.875)
    } else {
        Bounds::new(0.125, 0.0, 0.0, 0.875, 1.0, 1.0)
    }
}

/// A chest: inset a sixteenth on each side, reaching the block edge towards the first neighbour
/// (north, south, west, east) that is the same kind of chest.
///
/// The game's collision reads the chest class bounds without refreshing them, so every chest of a
/// kind collides with the shape of the chest that was rendered or targeted last (the single-chest
/// shape before any was). This model uses the queried chest's own neighbours.
pub(super) fn chest(world: &impl World, x: i32, y: i32, z: i32, id: u16) -> Bounds {
    let same = |x: i32, z: i32| world.block(x, y, z).id == id;
    if same(x, z.wrapping_sub(1)) {
        Bounds::new(0.0625, 0.0, 0.0, 0.9375, 0.875, 0.9375)
    } else if same(x, z.wrapping_add(1)) {
        Bounds::new(0.0625, 0.0, 0.0625, 0.9375, 0.875, 1.0)
    } else if same(x.wrapping_sub(1), z) {
        Bounds::new(0.0, 0.0, 0.0625, 0.9375, 0.875, 0.9375)
    } else if same(x.wrapping_add(1), z) {
        Bounds::new(0.0625, 0.0, 0.0625, 1.0, 0.875, 0.9375)
    } else {
        SINGLE_CHEST
    }
}

/// Farmland collides as the whole block, built from integer corners.
pub(super) fn farmland(x: i32, y: i32, z: i32) -> Aabb {
    Aabb::new(
        f64::from(x),
        f64::from(y),
        f64::from(z),
        f64::from(x.wrapping_add(1)),
        f64::from(y.wrapping_add(1)),
        f64::from(z.wrapping_add(1)),
    )
}

/// A snow layer: an eighth of a block per layer count in the low three metadata bits, so a
/// single layer has a zero-height box. The top is an `f32` sum with the block's Y.
pub(super) fn snow_layer(x: i32, y: i32, z: i32, meta: i32) -> Aabb {
    let layers = (meta & 7) as f32;
    let top = y as f32 + layers * EIGHTH;
    flat(x, y, z, top)
}

/// Carpet layers the collision box counts; 1.7.10 fixes it at zero, so the box has no height.
const CARPET_COLLISION_LAYERS: f32 = 0.0;

/// A carpet: a zero-height box on the block's floor, whose top is an `f32` sum with the block's Y.
pub(super) fn carpet(x: i32, y: i32, z: i32) -> Aabb {
    let top = y as f32 + CARPET_COLLISION_LAYERS * SIXTEENTH;
    flat(x, y, z, top)
}

/// A box covering the block's whole footprint from its floor up to the world height `top`.
fn flat(x: i32, y: i32, z: i32, top: f32) -> Aabb {
    let unit = Bounds::FULL;
    Aabb::new(
        f64::from(x) + unit.min_x,
        f64::from(y) + unit.min_y,
        f64::from(z) + unit.min_z,
        f64::from(x) + unit.max_x,
        f64::from(top),
        f64::from(z) + unit.max_z,
    )
}

/// A cactus: inset a sixteenth on the sides and top, with corners summed in `f32`.
pub(super) fn cactus(x: i32, y: i32, z: i32) -> Aabb {
    let inset = SIXTEENTH;
    Aabb::new(
        f64::from(x as f32 + inset),
        f64::from(y),
        f64::from(z as f32 + inset),
        f64::from(x.wrapping_add(1) as f32 - inset),
        f64::from(y.wrapping_add(1) as f32 - inset),
        f64::from(z.wrapping_add(1) as f32 - inset),
    )
}

/// Soul sand: an eighth short of the full block, with the top summed in `f32`.
pub(super) fn soul_sand(x: i32, y: i32, z: i32) -> Aabb {
    Aabb::new(
        f64::from(x),
        f64::from(y),
        f64::from(z),
        f64::from(x.wrapping_add(1)),
        f64::from(y.wrapping_add(1) as f32 - EIGHTH),
        f64::from(z.wrapping_add(1)),
    )
}

/// A cake: inset a sixteenth, with the west side receding two sixteenths per bite in the
/// metadata and the top a sixteenth below half height. Corners are summed in `f32`.
pub(super) fn cake(x: i32, y: i32, z: i32, meta: i32) -> Aabb {
    let inset = SIXTEENTH;
    let eaten = (1 + meta * 2) as f32 / 16.0;
    let height: f32 = 0.5;
    Aabb::new(
        f64::from(x as f32 + eaten),
        f64::from(y),
        f64::from(z as f32 + inset),
        f64::from(x.wrapping_add(1) as f32 - inset),
        f64::from(y as f32 + height - inset),
        f64::from(z.wrapping_add(1) as f32 - inset),
    )
}
