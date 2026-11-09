//! Where a rider stands after it gets off its vehicle.
//!
//! Vanilla tries the eight positions around the vehicle's block, first at the vehicle's
//! level, then a block up, then a block down, and takes the first one that has a floor and
//! fits the rider among the block shapes there. Without one the rider stays on its seat.

use crate::{Aabb, CollisionWorld, Vec3};

/// Lifts the placed feet clear of the floor under them.
const FLOOR_CLEARANCE: f32 = 0.001;
/// Riders wider than a block are tested a block wide.
const MAX_TEST_WIDTH: f32 = 1.0;
/// Squared horizontal vehicle motion at or below which the vehicle counts as still.
const STILL_MOTION_SQUARED: f32 = 1.0e-7;
/// Levels tried for each position, relative to the vehicle's block.
const LEVELS: [f32; 3] = [0.0, 1.0, -1.0];

/// The vehicle and rider a dismount places.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dismount {
    /// The vehicle's position.
    pub vehicle: Vec3,
    /// How far the vehicle moved over its last tick. Its larger horizontal axis decides
    /// which sides the rider tries first.
    pub vehicle_motion: Vec3,
    /// The rider's feet on its seat.
    pub seat: Vec3,
    pub rider_width: f64,
    pub rider_height: f64,
}

impl Dismount {
    /// The rider's feet once it is off the vehicle: beside the vehicle on the first free
    /// position with a floor, or its seat when none is free. Block shapes the world cannot
    /// attribute to a cell, such as actors, never block a position or make a floor. `None`
    /// until the world holds every block the search or the rider's first steps from it could
    /// touch, such as while their columns stream in.
    #[must_use]
    pub fn exit_feet(&self, world: &impl CollisionWorld) -> Option<Vec3> {
        let vehicle = self.vehicle.rounded();
        let base = [
            (vehicle.x as f32).floor() + 0.5,
            (vehicle.y as f32).floor(),
            (vehicle.z as f32).floor() + 0.5,
        ];
        // The search, and the rider's first steps from where it stands, stay within two
        // blocks of the vehicle's block.
        let around = Aabb::new(
            vec3([base[0] - 2.5, base[1] - 2.0, base[2] - 2.5]),
            vec3([
                base[0] + 2.5,
                base[1] + 2.0 + self.rider_height as f32,
                base[2] + 2.5,
            ]),
        );
        if world
            .collision_boxes_with_provenance(around)
            .ok()?
            .identity
            .reaches_absent_columns()
        {
            return None;
        }
        for level in LEVELS {
            for offset in self.candidates() {
                let probe = [base[0] + offset[0], base[1] + level, base[2] + offset[2]];
                let cell = probe.map(|axis| axis.floor() as i32);
                let Some(floor) = floor_height(world, cell) else {
                    continue;
                };
                if self.fits(world, [probe[0], probe[1] + floor, probe[2]]) {
                    // Vanilla tests the block-centred probe but places the rider relative to
                    // its seat, so a seat off its block's centre lands off the tested spot.
                    return Some(Vec3::new(
                        f64::from(self.seat.x as f32 + offset[0]),
                        f64::from(base[1] + floor + level + FLOOR_CLEARANCE),
                        f64::from(self.seat.z as f32 + offset[2]),
                    ));
                }
            }
        }
        Some(Vec3::new(
            self.seat.x,
            f64::from(self.seat.y as f32 + FLOOR_CLEARANCE),
            self.seat.z,
        ))
    }

    /// Horizontal offsets in trial order: the two sides across the vehicle's heading, the
    /// four diagonals, then behind and ahead of it. A still vehicle heads west.
    fn candidates(&self) -> [[f32; 3]; 8] {
        let [dx, dz] = [self.vehicle_motion.x as f32, self.vehicle_motion.z as f32];
        let ahead = if dx * dx + dz * dz <= STILL_MOTION_SQUARED {
            [-1.0, 0.0]
        } else if dz.abs() < dx.abs() {
            [dx.signum(), 0.0]
        } else {
            [0.0, dz.signum()]
        };
        let side = [-ahead[1], ahead[0]];
        let offset = |side_sign: f32, ahead_sign: f32| {
            [
                side[0] * side_sign + ahead[0] * ahead_sign,
                0.0,
                side[1] * side_sign + ahead[1] * ahead_sign,
            ]
        };
        [
            offset(1.0, 0.0),
            offset(-1.0, 0.0),
            offset(1.0, -1.0),
            offset(-1.0, -1.0),
            offset(1.0, 1.0),
            offset(-1.0, 1.0),
            offset(0.0, -1.0),
            offset(0.0, 1.0),
        ]
    }

    /// Whether the rider's box standing at `feet` overlaps no solid block shape. Each block
    /// counts as one box around all its shapes, and only blocks whose cells the rider's box
    /// covers are tested.
    fn fits(&self, world: &impl CollisionWorld, feet: [f32; 3]) -> bool {
        let half_width = (self.rider_width as f32).min(MAX_TEST_WIDTH) * 0.5;
        let rider = Aabb::new(
            vec3([feet[0] - half_width, feet[1], feet[2] - half_width]),
            vec3([
                feet[0] + half_width,
                feet[1] + self.rider_height as f32,
                feet[2] + half_width,
            ]),
        );
        let first = [rider.min.x, rider.min.y, rider.min.z].map(|axis| axis.floor() as i32);
        let last = [rider.max.x, rider.max.y, rider.max.z].map(|axis| axis.floor() as i32);
        let Some(blocks) = block_shapes(world, first, last) else {
            return false;
        };
        !blocks.iter().any(|(_, shape)| {
            !shape.passable
                && rider.min.x < shape.bounds.max.x
                && shape.bounds.min.x < rider.max.x
                && rider.min.y < shape.bounds.max.y
                && shape.bounds.min.y < rider.max.y
                && rider.min.z < shape.bounds.max.z
                && shape.bounds.min.z < rider.max.z
        })
    }
}

/// One block's collision shapes as a single box.
#[derive(Debug, Clone, Copy)]
struct BlockShape {
    bounds: Aabb,
    /// Scaffolding never blocks a dismount nor makes its floor.
    passable: bool,
}

/// Height of the floor in `cell` above the cell's base: the top of a block lower than a full
/// block there, or the top of a full-height block below an empty cell.
fn floor_height(world: &impl CollisionWorld, cell: [i32; 3]) -> Option<f32> {
    let below = [cell[0], cell[1] - 1, cell[2]];
    let blocks = block_shapes(world, below, cell)?;
    let shape = |at| {
        blocks
            .iter()
            .find(|(block, _)| *block == at)
            .map(|(_, s)| *s)
    };
    let base = cell[1] as f32;
    if let Some(own) = shape(cell) {
        let top = own.bounds.max.y as f32 - base;
        if top < 1.0 {
            return Some(top);
        }
        if !own.passable {
            return None;
        }
    }
    let below = shape(below).filter(|shape| !shape.passable)?;
    let top = below.bounds.max.y as f32 - base;
    (top >= 0.0).then_some(top)
}

/// The shapes of each block in the cells from `first` to `last`, or `None` when the world
/// cannot answer.
fn block_shapes(
    world: &impl CollisionWorld,
    first: [i32; 3],
    last: [i32; 3],
) -> Option<Vec<([i32; 3], BlockShape)>> {
    let query = Aabb::new(
        vec3(first.map(|axis| axis as f32)),
        vec3(last.map(|axis| (axis + 1) as f32)),
    );
    let colliders = world.collision_boxes_with_provenance(query).ok()?.value;
    let mut blocks: Vec<([i32; 3], BlockShape)> = Vec::new();
    for collider in colliders {
        let Some(block) = collider.block else {
            continue;
        };
        if (0..3).any(|axis| block[axis] < first[axis] || block[axis] > last[axis]) {
            continue;
        }
        let passable = collider.cell_has_scaffolding == Some(true);
        match blocks.iter_mut().find(|(cell, _)| *cell == block) {
            Some((_, shape)) => {
                shape.bounds = Aabb::new(
                    shape.bounds.min.component_min(collider.aabb.min),
                    shape.bounds.max.component_max(collider.aabb.max),
                );
                shape.passable |= passable;
            }
            None => blocks.push((
                block,
                BlockShape {
                    bounds: collider.aabb,
                    passable,
                },
            )),
        }
    }
    Some(blocks)
}

fn vec3([x, y, z]: [f32; 3]) -> Vec3 {
    Vec3::new(f64::from(x), f64::from(y), f64::from(z))
}

#[cfg(test)]
#[path = "dismount_tests.rs"]
mod tests;
