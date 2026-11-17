//! Doors, trapdoors and fence gates.

use super::{Bounds, meta_of};
use crate::math::Aabb;
use crate::world::World;

/// Thickness of a door or trapdoor leaf.
const LEAF: f32 = 0.1875;

/// A leaf against the west face of the block.
const WEST: Bounds = Bounds::new(0.0, 0.0, 0.0, LEAF, 1.0, 1.0);
/// A leaf against the east face of the block.
const EAST: Bounds = Bounds::new(1.0 - LEAF, 0.0, 0.0, 1.0, 1.0, 1.0);
/// A leaf against the north face of the block.
const NORTH: Bounds = Bounds::new(0.0, 0.0, 0.0, 1.0, 1.0, LEAF);
/// A leaf against the south face of the block.
const SOUTH: Bounds = Bounds::new(0.0, 0.0, 1.0 - LEAF, 1.0, 1.0, 1.0);

/// A door half's leaf.
///
/// Facing and the open bit come from the lower half's metadata and the hinge side from the upper
/// half's, so each half reads the block above or below it, whatever that block is.
pub(super) fn door(world: &impl World, x: i32, y: i32, z: i32, meta: i32) -> Bounds {
    let is_upper = meta & 8 != 0;
    let (lower, upper) = if is_upper {
        (meta_of(world.block(x, y.wrapping_sub(1), z)), meta)
    } else {
        (meta, meta_of(world.block(x, y.wrapping_add(1), z)))
    };
    let facing = lower & 3;
    let open = lower & 4 != 0;
    let hinge_flipped = upper & 1 != 0;
    match (facing, open, hinge_flipped) {
        (0, false, _) | (1, true, true) | (3, true, false) => WEST,
        (1, false, _) | (0, true, false) | (2, true, true) => NORTH,
        (2, false, _) | (1, true, false) | (3, true, true) => EAST,
        _ => SOUTH,
    }
}

/// A trapdoor: flat against the top (metadata bit 8) or bottom of the block when closed, or
/// upright against the side its low two bits name when open (bit 4).
pub(super) fn trapdoor(meta: i32) -> Bounds {
    if meta & 4 != 0 {
        return match meta & 3 {
            0 => SOUTH,
            1 => NORTH,
            2 => EAST,
            _ => WEST,
        };
    }
    if meta & 8 != 0 {
        Bounds::new(0.0, 1.0 - LEAF, 0.0, 1.0, 1.0, 1.0)
    } else {
        Bounds::new(0.0, 0.0, 0.0, 1.0, LEAF, 1.0)
    }
}

/// A closed fence gate: a quarter-thick, one-and-a-half-high bar; an open gate (metadata bit 4)
/// has no box.
///
/// The bar runs along X only when the whole metadata is 0 or 2, and along Z for every other
/// closed value. The thin axis and the top are `f32` sums with the block position.
pub(super) fn fence_gate(x: i32, y: i32, z: i32, meta: i32) -> Option<Aabb> {
    if meta & 4 != 0 {
        return None;
    }
    let top = f64::from(y as f32 + 1.5);
    Some(if meta == 2 || meta == 0 {
        Aabb::new(
            f64::from(x),
            f64::from(y),
            f64::from(z as f32 + 0.375),
            f64::from(x.wrapping_add(1)),
            top,
            f64::from(z as f32 + 0.625),
        )
    } else {
        Aabb::new(
            f64::from(x as f32 + 0.375),
            f64::from(y),
            f64::from(z),
            f64::from(x as f32 + 0.625),
            top,
            f64::from(z.wrapping_add(1)),
        )
    })
}
