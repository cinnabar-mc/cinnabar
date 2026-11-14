//! Moving the player box through the world: sneak edges, axis clipping, stepping, and the
//! effects of the blocks it ends up touching.

use super::{EYE_OFFSET, Player, STEP_HEIGHT};
use crate::block::{self, collision_boxes};
use crate::math::{Aabb, floor};
use crate::world::World;

/// Step a sneaking player's motion shrinks by while it looks for ground under its edge.
const SNEAK_EDGE_STEP: f64 = 0.05;
/// Share of the camera sink kept each move.
const SINK_DECAY: f32 = 0.4;
/// A camera sink below this allows stepping up.
const STEP_SINK_LIMIT: f32 = 0.05;
/// Motion scale inside a cobweb.
const WEB_HORIZONTAL_SCALE: f64 = 0.25;
const WEB_VERTICAL_SCALE: f32 = 0.05;
/// Horizontal motion kept per soul sand block touched.
const SOUL_SAND_SCALE: f64 = 0.4;
/// Margin entities are searched in around a collision query.
const ENTITY_QUERY_MARGIN: f64 = 0.25;
/// Longest move along one axis in a tick, in blocks.
const MAX_STEP: f64 = 100.0;
/// Inset of the box when finding the blocks it touches.
const TOUCH_INSET: f64 = 0.001;

impl Player {
    /// Block and entity boxes that can collide with `query`, in the game's scan order: X, then
    /// Z, then Y from one block below the query.
    pub(super) fn colliding_boxes(&self, world: &impl World, query: &Aabb) -> Vec<Aabb> {
        let mut boxes = Vec::new();
        let (min_x, max_x) = (floor(query.min_x), floor(query.max_x + 1.0));
        let (min_y, max_y) = (floor(query.min_y), floor(query.max_y + 1.0));
        let (min_z, max_z) = (floor(query.min_z), floor(query.max_z + 1.0));
        for x in min_x..max_x {
            for z in min_z..max_z {
                if !world.block_exists(x, 64, z) {
                    continue;
                }
                for y in min_y.wrapping_sub(1)..max_y {
                    collision_boxes(world, x, y, z, query, &mut boxes);
                }
            }
        }
        let mut entities = Vec::new();
        world.entity_boxes(
            &query.expand(
                ENTITY_QUERY_MARGIN,
                ENTITY_QUERY_MARGIN,
                ENTITY_QUERY_MARGIN,
            ),
            &mut entities,
        );
        boxes.extend(
            entities
                .into_iter()
                .filter(|entity| entity.intersects(query)),
        );
        boxes
    }

    /// Raises the player onto a surface its feet are sunk into by at most `tolerance` blocks.
    ///
    /// The game places a teleported player at the exact position the server sent. A caller
    /// that only has that position in single precision can land the feet a hair inside the
    /// ground, where the game's collision would let the player fall through.
    pub fn lift_onto_surface(&mut self, world: &impl World, tolerance: f64) {
        let feet = self.bounds.min_y;
        let surface = self
            .colliding_boxes(world, &self.bounds)
            .iter()
            .map(|obstacle| obstacle.max_y)
            .filter(|top| *top > feet && *top - feet <= tolerance)
            .fold(None, |highest: Option<f64>, top| {
                Some(highest.map_or(top, |held| held.max(top)))
            });
        if let Some(top) = surface {
            self.bounds.offset(0.0, top - feet, 0.0);
            self.y = self.bounds.min_y + f64::from(EYE_OFFSET) - f64::from(self.sink);
        }
    }

    /// Moves by `motion` as far as collisions allow, then updates ground and collision flags,
    /// fall distance, and the motion that hit something.
    pub(super) fn move_by(&mut self, world: &impl World, motion: [f64; 3]) {
        let [mut x, mut y, mut z] = motion.map(bounded_step);
        self.sink *= SINK_DECAY;
        if self.in_web {
            self.in_web = false;
            x *= WEB_HORIZONTAL_SCALE;
            y *= f64::from(WEB_VERTICAL_SCALE);
            z *= WEB_HORIZONTAL_SCALE;
            self.motion = [0.0; 3];
        }
        let start = self.bounds;
        let sneaking = self.on_ground && self.keys.sneak;
        if sneaking {
            (x, z) = self.clip_to_sneak_edge(world, x, z);
        }
        let (wanted_x, wanted_y, wanted_z) = (x, y, z);

        let boxes = self.colliding_boxes(world, &self.bounds.add_coord(x, y, z));
        (x, y, z) = self.clip_and_move(&boxes, x, y, z);
        let on_ground_before = self.on_ground || (wanted_y != y && wanted_y < 0.0);

        if STEP_HEIGHT > 0.0
            && on_ground_before
            && (sneaking || self.sink < STEP_SINK_LIMIT)
            && (wanted_x != x || wanted_z != z)
        {
            let flat_bounds = self.bounds;
            self.bounds = start;
            let step = f64::from(STEP_HEIGHT);
            let boxes =
                self.colliding_boxes(world, &self.bounds.add_coord(wanted_x, step, wanted_z));
            let (stepped_x, _, stepped_z) = self.clip_and_move(&boxes, wanted_x, step, wanted_z);
            // Settle back down by up to the step height.
            let mut settled_y = -step;
            for obstacle in &boxes {
                settled_y = obstacle.clip_y(&self.bounds, settled_y);
            }
            self.bounds.offset(0.0, settled_y, 0.0);
            if x * x + z * z >= stepped_x * stepped_x + stepped_z * stepped_z {
                self.bounds = flat_bounds;
            } else {
                (x, y, z) = (stepped_x, settled_y, stepped_z);
            }
        }

        self.x = (self.bounds.min_x + self.bounds.max_x) / 2.0;
        self.y = self.bounds.min_y + f64::from(EYE_OFFSET) - f64::from(self.sink);
        self.z = (self.bounds.min_z + self.bounds.max_z) / 2.0;
        self.collided_horizontally = wanted_x != x || wanted_z != z;
        self.collided_vertically = wanted_y != y;
        self.on_ground = wanted_y != y && wanted_y < 0.0;
        self.update_fall_state(world, y);
        if wanted_x != x {
            self.motion[0] = 0.0;
        }
        if wanted_y != y {
            self.motion[1] = 0.0;
        }
        if wanted_z != z {
            self.motion[2] = 0.0;
        }
        self.touch_blocks(world);
    }

    /// Clips the motion against `boxes` on Y, then X, then Z, moving the box after each axis.
    fn clip_and_move(
        &mut self,
        boxes: &[Aabb],
        mut x: f64,
        mut y: f64,
        mut z: f64,
    ) -> (f64, f64, f64) {
        for obstacle in boxes {
            y = obstacle.clip_y(&self.bounds, y);
        }
        self.bounds.offset(0.0, y, 0.0);
        for obstacle in boxes {
            x = obstacle.clip_x(&self.bounds, x);
        }
        self.bounds.offset(x, 0.0, 0.0);
        for obstacle in boxes {
            z = obstacle.clip_z(&self.bounds, z);
        }
        self.bounds.offset(0.0, 0.0, z);
        (x, y, z)
    }

    /// Shortens horizontal motion so a sneaking player keeps ground within one block below.
    fn clip_to_sneak_edge(&self, world: &impl World, mut x: f64, mut z: f64) -> (f64, f64) {
        let unsupported = |x: f64, z: f64| {
            self.colliding_boxes(world, &self.bounds.offset_by(x, -1.0, z))
                .is_empty()
        };
        while x != 0.0 && unsupported(x, 0.0) {
            x = shorten(x);
        }
        while z != 0.0 && unsupported(0.0, z) {
            z = shorten(z);
        }
        while x != 0.0 && z != 0.0 && unsupported(x, z) {
            x = shorten(x);
            z = shorten(z);
        }
        (x, z)
    }

    /// Fall bookkeeping after a move: entering water clears the fall, landing ends it, and
    /// falling adds the distance dropped.
    fn update_fall_state(&mut self, world: &impl World, moved_y: f64) {
        if !self.in_water {
            self.handle_water_movement(world);
        }
        if self.on_ground {
            self.fall_distance = 0.0;
        } else if moved_y < 0.0 {
            self.fall_distance = (f64::from(self.fall_distance) - moved_y) as f32;
        }
    }

    /// Applies the blocks the box overlaps: cobwebs slow the next move, unless the player is
    /// flying, and soul sand slows horizontal motion once per block.
    fn touch_blocks(&mut self, world: &impl World) {
        let min = [
            floor(self.bounds.min_x + TOUCH_INSET),
            floor(self.bounds.min_y + TOUCH_INSET),
            floor(self.bounds.min_z + TOUCH_INSET),
        ];
        let max = [
            floor(self.bounds.max_x - TOUCH_INSET),
            floor(self.bounds.max_y - TOUCH_INSET),
            floor(self.bounds.max_z - TOUCH_INSET),
        ];
        if !world.chunks_exist(min, max) {
            return;
        }
        for x in min[0]..=max[0] {
            for y in min[1]..=max[1] {
                for z in min[2]..=max[2] {
                    match world.block(x, y, z).id {
                        block::id::WEB if !self.abilities.flying => {
                            self.in_web = true;
                            self.fall_distance = 0.0;
                        }
                        block::id::SOUL_SAND => {
                            self.motion[0] *= SOUL_SAND_SCALE;
                            self.motion[2] *= SOUL_SAND_SCALE;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Moves a motion component one sneak-edge step toward zero, stopping at zero.
fn shorten(value: f64) -> f64 {
    if (-SNEAK_EDGE_STEP..SNEAK_EDGE_STEP).contains(&value) {
        0.0
    } else if value > 0.0 {
        value - SNEAK_EDGE_STEP
    } else {
        value + SNEAK_EDGE_STEP
    }
}

/// Limits one component of a move to [`MAX_STEP`] and treats a non-number as no movement.
/// The game has no such limit: a larger move makes it scan every block on the way, and one
/// too large for the sneak step to shorten never ends. Servers reject moves a tenth this size.
fn bounded_step(value: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(-MAX_STEP, MAX_STEP)
    }
}
