//! Water and lava queries: whether the player is in a liquid, and the current that pushes it.

use super::Player;
use crate::block::{Material, facts};
use crate::math::{Aabb, floor, sqrt};
use crate::world::World;

/// Vertical inset of the box when testing for water and lava.
const LIQUID_VERTICAL_INSET: f32 = -0.4;
/// Horizontal inset of the box when testing for lava.
const LAVA_HORIZONTAL_INSET: f32 = -0.1;
const WATER_BOX_MARGIN: f64 = 0.001;
/// Speed a water current adds per tick.
const CURRENT_ACCELERATION: f64 = 0.014;
/// Downward pull of falling water held against a solid side.
const FALLING_WATER_PULL: f64 = -6.0;

impl Player {
    /// Updates whether the player is in water; water pushes it along its current (unless it
    /// is flying) and clears its fall distance.
    pub(super) fn handle_water_movement(&mut self, world: &impl World) {
        let query = self
            .bounds
            .expand(0.0, f64::from(LIQUID_VERTICAL_INSET), 0.0)
            .contract(WATER_BOX_MARGIN, WATER_BOX_MARGIN, WATER_BOX_MARGIN);
        match water_current(world, &query) {
            Some(current) => {
                if !self.abilities.flying {
                    let length = f64::from(sqrt(
                        current[0] * current[0] + current[1] * current[1] + current[2] * current[2],
                    ));
                    if length > 0.0 {
                        let direction = normalize(current);
                        self.motion[0] += direction[0] * CURRENT_ACCELERATION;
                        self.motion[1] += direction[1] * CURRENT_ACCELERATION;
                        self.motion[2] += direction[2] * CURRENT_ACCELERATION;
                    }
                }
                self.fall_distance = 0.0;
                self.in_water = true;
            }
            None => self.in_water = false,
        }
    }
}

/// The block range a liquid query scans: each axis from the floored minimum up to, but not
/// including, the floored maximum plus one.
fn scan_range(query: &Aabb) -> ([i32; 3], [i32; 3]) {
    (
        [floor(query.min_x), floor(query.min_y), floor(query.min_z)],
        [
            floor(query.max_x + 1.0),
            floor(query.max_y + 1.0),
            floor(query.max_z + 1.0),
        ],
    )
}

/// The summed current of the water blocks whose surface reaches into `query`, or `None` when
/// the box touches no water or a column in range is not loaded.
fn water_current(world: &impl World, query: &Aabb) -> Option<[f64; 3]> {
    let (min, end) = scan_range(query);
    if !world.chunks_exist(min, end) {
        return None;
    }
    let mut found = false;
    let mut current = [0.0_f64; 3];
    for x in min[0]..end[0] {
        for y in min[1]..end[1] {
            for z in min[2]..end[2] {
                let block = world.block(x, y, z);
                if facts(block.id).material != Material::Water {
                    continue;
                }
                let surface = f64::from(y.wrapping_add(1) as f32 - liquid_height(block.meta));
                if f64::from(end[1]) >= surface {
                    found = true;
                    let flow = flow_vector(world, x, y, z, Material::Water);
                    current[0] += flow[0];
                    current[1] += flow[1];
                    current[2] += flow[2];
                }
            }
        }
    }
    found.then_some(current)
}

/// Whether the player's box, inset for lava, overlaps a lava block.
pub(super) fn touches_lava(world: &impl World, bounds: &Aabb) -> bool {
    let inset = f64::from(LAVA_HORIZONTAL_INSET);
    let query = bounds.expand(inset, f64::from(LIQUID_VERTICAL_INSET), inset);
    let (min, end) = scan_range(&query);
    (min[0]..end[0]).any(|x| {
        (min[1]..end[1]).any(|y| {
            (min[2]..end[2]).any(|z| facts(world.block(x, y, z).id).material == Material::Lava)
        })
    })
}

/// Whether any liquid block lies in the range of `query`; negative minimums widen the range by
/// one block, as the game's scan does.
pub(super) fn any_liquid(world: &impl World, query: &Aabb) -> bool {
    let (mut min, end) = scan_range(query);
    if query.min_x < 0.0 {
        min[0] -= 1;
    }
    if query.min_y < 0.0 {
        min[1] -= 1;
    }
    if query.min_z < 0.0 {
        min[2] -= 1;
    }
    (min[0]..end[0]).any(|x| {
        (min[1]..end[1]).any(|y| (min[2]..end[2]).any(|z| facts(world.block(x, y, z).id).liquid))
    })
}

/// Share of a block a liquid of this metadata leaves empty above its surface: sources and
/// falling liquid count as level zero.
fn liquid_height(meta: u8) -> f32 {
    let level = if meta >= 8 { 0 } else { meta };
    f32::from(level + 1) / 9.0
}

/// Flow level of the liquid at a position: 0 for sources and falling liquid, up to 7, or
/// `None` when the block is not that liquid.
fn flow_level(world: &impl World, x: i32, y: i32, z: i32, liquid: Material) -> Option<i32> {
    let block = world.block(x, y, z);
    if facts(block.id).material != liquid {
        return None;
    }
    Some(if block.meta >= 8 {
        0
    } else {
        i32::from(block.meta)
    })
}

/// Whether a neighbour holds falling liquid against it on a side: any block of a solid
/// material except the liquid itself and ice; the top side always does.
fn holds_liquid(world: &impl World, x: i32, y: i32, z: i32, liquid: Material, top: bool) -> bool {
    let material = facts(world.block(x, y, z).id);
    if material.material == liquid {
        return false;
    }
    if top {
        return true;
    }
    if material.material == Material::Ice {
        return false;
    }
    material.solid
}

/// The direction a liquid block flows: toward lower neighbours, and downward where falling
/// liquid runs along a solid side. The result has unit length or is zero.
fn flow_vector(world: &impl World, x: i32, y: i32, z: i32, liquid: Material) -> [f64; 3] {
    let mut flow = [0.0_f64; 3];
    let level = flow_level(world, x, y, z, liquid).unwrap_or(-1);
    for (offset_x, offset_z) in [(-1, 0), (0, -1), (1, 0), (0, 1)] {
        let (neighbour_x, neighbour_z) = (x.wrapping_add(offset_x), z.wrapping_add(offset_z));
        let weight = match flow_level(world, neighbour_x, y, neighbour_z, liquid) {
            Some(neighbour) => neighbour - level,
            None => {
                if facts(world.block(neighbour_x, y, neighbour_z).id).blocks_movement {
                    continue;
                }
                match flow_level(world, neighbour_x, y.wrapping_sub(1), neighbour_z, liquid) {
                    Some(below) => below - (level - 8),
                    None => continue,
                }
            }
        };
        flow[0] += f64::from(offset_x * weight);
        flow[2] += f64::from(offset_z * weight);
    }
    if world.block(x, y, z).meta >= 8 {
        let held = [
            (0, 0, -1),
            (0, 0, 1),
            (-1, 0, 0),
            (1, 0, 0),
            (0, 1, -1),
            (0, 1, 1),
            (-1, 1, 0),
            (1, 1, 0),
        ]
        .into_iter()
        .any(|(offset_x, offset_y, offset_z)| {
            holds_liquid(
                world,
                x.wrapping_add(offset_x),
                y.wrapping_add(offset_y),
                z.wrapping_add(offset_z),
                liquid,
                false,
            )
        });
        if held {
            flow = normalize(flow);
            flow[1] += FALLING_WATER_PULL;
        }
    }
    normalize(flow)
}

/// Scales a vector to unit length; vectors shorter than 0.0001 become zero.
fn normalize(vector: [f64; 3]) -> [f64; 3] {
    let length = f64::from(sqrt(
        vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2],
    ));
    if length < 1.0e-4 {
        return [0.0; 3];
    }
    [vector[0] / length, vector[1] / length, vector[2] / length]
}
