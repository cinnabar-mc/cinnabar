use std::collections::HashMap;

use super::Dismount;
use crate::{
    Aabb, CollisionQuery, CollisionWorld, PLAYER_HEIGHT, PLAYER_WIDTH, ProvenancedCollider, Vec3,
    WorldQueryError,
};

/// Ground top in every test: the grass layer fills the cells at `GROUND - 1`.
const GROUND: i32 = -60;

/// Blocks by cell, each with shapes in block-local coordinates, plus actor boxes that no cell
/// owns.
#[derive(Default)]
struct World {
    blocks: HashMap<[i32; 3], (Vec<Aabb>, bool)>,
    actors: Vec<Aabb>,
}

impl World {
    /// A flat grass floor around the origin.
    fn ground() -> Self {
        let mut world = Self::default();
        for x in -4..=4 {
            for z in -4..=4 {
                world.solid([x, GROUND - 1, z]);
            }
        }
        world
    }

    fn solid(&mut self, cell: [i32; 3]) {
        self.shaped(cell, Aabb::new(Vec3::ZERO, Vec3::ONE));
    }

    fn shaped(&mut self, cell: [i32; 3], shape: Aabb) {
        self.blocks.insert(cell, (vec![shape], false));
    }

    fn scaffolding(&mut self, cell: [i32; 3]) {
        self.blocks
            .insert(cell, (vec![Aabb::new(Vec3::ZERO, Vec3::ONE)], true));
    }
}

impl CollisionWorld for World {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            self.collision_boxes_with_provenance(query)?
                .value
                .into_iter()
                .map(|collider| collider.aabb)
                .collect(),
        ))
    }

    fn collision_boxes_with_provenance(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<Vec<ProvenancedCollider>>, WorldQueryError> {
        let mut colliders = Vec::new();
        for (cell, (shapes, scaffolding)) in &self.blocks {
            let origin = Vec3::new(f64::from(cell[0]), f64::from(cell[1]), f64::from(cell[2]));
            for shape in shapes {
                let aabb = Aabb::new(shape.min + origin, shape.max + origin);
                if aabb.intersects(query) {
                    colliders.push(ProvenancedCollider {
                        aabb,
                        block: Some(*cell),
                        runtime_id: None,
                        cell_has_scaffolding: Some(*scaffolding),
                    });
                }
            }
        }
        for aabb in &self.actors {
            if aabb.intersects(query) {
                colliders.push(ProvenancedCollider {
                    aabb: *aabb,
                    block: None,
                    runtime_id: None,
                    cell_has_scaffolding: None,
                });
            }
        }
        Ok(CollisionQuery::synthetic(colliders))
    }
}

/// A boat resting on the ground in the block at the origin, its rider's seat sunk below the
/// ground top as a boat's seat is.
fn still_boat() -> Dismount {
    let ground = f64::from(GROUND);
    Dismount {
        vehicle: Vec3::new(0.5, ground, 0.5),
        vehicle_motion: Vec3::ZERO,
        seat: Vec3::new(0.5, ground - 0.225, 0.5),
        rider_width: PLAYER_WIDTH,
        rider_height: PLAYER_HEIGHT,
    }
}

fn assert_feet(actual: Vec3, expected: [f32; 3]) {
    let actual = [actual.x as f32, actual.y as f32, actual.z as f32];
    for axis in 0..3 {
        assert!(
            (actual[axis] - expected[axis]).abs() < 1.0e-4,
            "{actual:?} != {expected:?}"
        );
    }
}

fn wall(world: &mut World, x: i32, z: i32) {
    world.solid([x, GROUND, z]);
    world.solid([x, GROUND + 1, z]);
}

#[test]
fn a_rider_leaving_a_boat_on_land_stands_on_the_ground_beside_it() {
    let exit = still_boat().exit_feet(&World::ground()).unwrap();
    // A still vehicle tries north first, and the rider stands on the ground top.
    assert_feet(exit, [0.5, GROUND as f32 + 0.001, -0.5]);
}

#[test]
fn a_blocked_side_moves_the_rider_to_the_next_free_side() {
    let mut world = World::ground();
    wall(&mut world, 0, -1);
    assert_feet(
        still_boat().exit_feet(&world).unwrap(),
        [0.5, GROUND as f32 + 0.001, 1.5],
    );
}

#[test]
fn sides_are_tried_across_the_heading_then_diagonally_then_behind_and_ahead() {
    let mut world = World::ground();
    let mut exits = Vec::new();
    for _ in 0..8 {
        let exit = still_boat().exit_feet(&world).unwrap();
        let offset = [(exit.x - 0.5).round() as i32, (exit.z - 0.5).round() as i32];
        exits.push(offset);
        wall(&mut world, offset[0], offset[1]);
    }
    // A still vehicle heads west: sides north and south, then the diagonals, behind (east)
    // and ahead (west).
    assert_eq!(
        exits,
        [
            [0, -1],
            [0, 1],
            [1, -1],
            [1, 1],
            [-1, -1],
            [-1, 1],
            [1, 0],
            [-1, 0]
        ]
    );
}

#[test]
fn a_moving_vehicle_puts_its_rider_across_its_heading() {
    let mut boat = still_boat();
    boat.vehicle_motion = Vec3::new(0.3, 0.0, 0.1);
    // Heading east, the first side is south.
    assert_feet(
        boat.exit_feet(&World::ground()).unwrap(),
        [0.5, GROUND as f32 + 0.001, 1.5],
    );
    boat.vehicle_motion = Vec3::new(0.1, 0.0, -0.3);
    // Heading north, the first side is east.
    assert_feet(
        boat.exit_feet(&World::ground()).unwrap(),
        [1.5, GROUND as f32 + 0.001, 0.5],
    );
}

#[test]
fn walled_in_at_its_level_the_rider_climbs_onto_the_walls() {
    let mut world = World::ground();
    for x in -1..=1 {
        for z in -1..=1 {
            if [x, z] != [0, 0] {
                world.solid([x, GROUND, z]);
            }
        }
    }
    assert_feet(
        still_boat().exit_feet(&world).unwrap(),
        [0.5, GROUND as f32 + 1.001, -0.5],
    );
}

#[test]
fn a_low_block_beside_the_vehicle_is_the_floor() {
    let mut world = World::ground();
    world.shaped(
        [0, GROUND, -1],
        Aabb::new(Vec3::ZERO, Vec3::new(1.0, 0.5, 1.0)),
    );
    assert_feet(
        still_boat().exit_feet(&world).unwrap(),
        [0.5, GROUND as f32 + 0.501, -0.5],
    );
}

#[test]
fn without_a_floor_beside_it_the_rider_stays_on_its_seat() {
    // Open water: nothing solid within a block of the boat's level.
    let mut boat = still_boat();
    boat.vehicle.y = f64::from(GROUND + 3);
    boat.seat.y = f64::from(GROUND + 3) - 0.225;
    let exit = boat.exit_feet(&World::ground()).unwrap();
    assert_feet(exit, [0.5, (GROUND + 3) as f32 - 0.224, 0.5]);
}

#[test]
fn actors_and_scaffolding_neither_block_nor_floor_a_position() {
    let mut world = World::ground();
    world.actors.push(Aabb::new(
        Vec3::new(0.0, f64::from(GROUND), -1.0),
        Vec3::new(1.0, f64::from(GROUND + 2), 0.0),
    ));
    world.scaffolding([0, GROUND, -1]);
    assert_feet(
        still_boat().exit_feet(&world).unwrap(),
        [0.5, GROUND as f32 + 0.001, -0.5],
    );
    // A floor of scaffolding alone holds no rider.
    world.blocks.remove(&[0, GROUND, -1]);
    world.blocks.remove(&[0, GROUND - 1, -1]);
    world.scaffolding([0, GROUND - 1, -1]);
    assert_feet(
        still_boat().exit_feet(&world).unwrap(),
        [0.5, GROUND as f32 + 0.001, 1.5],
    );
}

#[test]
fn a_taller_rider_skips_a_side_too_low_for_it() {
    let mut world = World::ground();
    world.solid([0, GROUND + 2, -1]);
    assert_feet(
        still_boat().exit_feet(&world).unwrap(),
        [0.5, GROUND as f32 + 0.001, -0.5],
    );
    let mut tall = still_boat();
    tall.rider_height = 2.4;
    assert_feet(
        tall.exit_feet(&world).unwrap(),
        [0.5, GROUND as f32 + 0.001, 1.5],
    );
}

/// A world that cannot answer for the blocks around the vehicle yet places no one.
#[test]
fn blocks_still_streaming_in_place_no_one() {
    struct Streaming;
    impl CollisionWorld for Streaming {
        fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            Err(WorldQueryError::QueryExtentExceeded)
        }
    }
    assert_eq!(still_boat().exit_feet(&Streaming), None);
}
