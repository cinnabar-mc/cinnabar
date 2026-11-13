//! Jumping and the per-medium movement step: water, lava, ground and air, and flight.

use super::{Player, Status, fluid};
use crate::block::{self, facts};
use crate::math::{cos, floor, sin};
use crate::world::World;

/// Motion components below this magnitude stop.
const MOTION_EPSILON: f64 = 0.005;
/// Ticks before a held jump key jumps again.
const JUMP_DELAY_TICKS: i32 = 10;
/// Input keeps this share each tick before it accelerates the player.
const INPUT_DAMPING: f32 = 0.98;
const JUMP_VELOCITY: f32 = 0.42;
/// Horizontal impulse a sprinting jump adds along the view.
const SPRINT_JUMP_IMPULSE: f32 = 0.2;
/// Upward acceleration per tick while the jump key is held in a liquid.
const SWIM_ACCELERATION: f32 = 0.04;
/// Acceleration from input in water and lava.
const LIQUID_ACCELERATION: f32 = 0.02;
const WATER_DRAG: f32 = 0.8;
const LAVA_DRAG: f64 = 0.5;
const LIQUID_GRAVITY: f64 = 0.02;
/// Height a swimmer must be able to rise to climb out, and the speed it then gets.
const LIQUID_EXIT_RISE: f32 = 0.6;
const LIQUID_EXIT_VELOCITY: f32 = 0.3;
const AIR_FRICTION: f32 = 0.91;
/// Ground acceleration at the default friction, which other frictions scale by their cube.
const GROUND_ACCELERATION: f32 = 0.16277136;
const LADDER_MAX_SPEED: f32 = 0.15;
const LADDER_MAX_FALL: f64 = -0.15;
const LADDER_CLIMB_VELOCITY: f64 = 0.2;
const GRAVITY: f64 = 0.08;
const VERTICAL_DRAG: f32 = 0.98;
/// Fall speed in a column that is not loaded.
const UNLOADED_FALL: f64 = -0.1;
/// Vertical motion kept each tick while flying.
const FLIGHT_VERTICAL_DAMPING: f64 = 0.6;
const DEGREES_TO_RADIANS: f32 = std::f32::consts::PI / 180.0;

impl Player {
    /// Applies input, jumping and one movement step.
    pub(super) fn travel_tick(&mut self, world: &impl World, status: &Status) {
        if self.jump_ticks > 0 {
            self.jump_ticks -= 1;
        }
        for component in &mut self.motion {
            if component.abs() < MOTION_EPSILON {
                *component = 0.0;
            }
        }
        if status.movement_blocked {
            self.jumping = false;
            self.move_strafe = 0.0;
            self.move_forward = 0.0;
        } else {
            self.move_strafe = self.keys.strafe;
            self.move_forward = self.keys.forward;
            self.jumping = self.keys.jump;
        }
        if self.jumping {
            if self.in_water || fluid::touches_lava(world, &self.bounds) {
                self.motion[1] += f64::from(SWIM_ACCELERATION);
            } else if self.on_ground && self.jump_ticks == 0 {
                self.jump(status);
                self.jump_ticks = JUMP_DELAY_TICKS;
            }
        } else {
            self.jump_ticks = 0;
        }
        self.move_strafe *= INPUT_DAMPING;
        self.move_forward *= INPUT_DAMPING;
        let (strafe, forward) = (self.move_strafe, self.move_forward);
        if self.abilities.flying {
            let vertical = self.motion[1];
            let air_speed = self.air_speed;
            self.air_speed = self.abilities.fly_speed;
            self.travel(world, strafe, forward);
            self.motion[1] = vertical * FLIGHT_VERTICAL_DAMPING;
            self.air_speed = air_speed;
        } else {
            self.travel(world, strafe, forward);
        }
    }

    /// Leaves the ground: Jump Boost adds height and sprinting adds speed along the view.
    fn jump(&mut self, status: &Status) {
        self.motion[1] = f64::from(JUMP_VELOCITY);
        if let Some(amplifier) = status.effects.jump_boost {
            self.motion[1] += f64::from((i32::from(amplifier) + 1) as f32 * 0.1);
        }
        if self.sprinting {
            let radians = self.yaw * DEGREES_TO_RADIANS;
            self.motion[0] -= f64::from(sin(radians) * SPRINT_JUMP_IMPULSE);
            self.motion[2] += f64::from(cos(radians) * SPRINT_JUMP_IMPULSE);
        }
    }

    /// One movement step in the medium the player is in.
    fn travel(&mut self, world: &impl World, strafe: f32, forward: f32) {
        if self.in_water && !self.abilities.flying {
            let start_y = self.y;
            self.accelerate(strafe, forward, LIQUID_ACCELERATION);
            self.move_by(world, self.motion);
            for component in &mut self.motion {
                *component *= f64::from(WATER_DRAG);
            }
            self.motion[1] -= LIQUID_GRAVITY;
            self.leave_liquid_at_bank(world, start_y);
        } else if fluid::touches_lava(world, &self.bounds) && !self.abilities.flying {
            let start_y = self.y;
            self.accelerate(strafe, forward, LIQUID_ACCELERATION);
            self.move_by(world, self.motion);
            for component in &mut self.motion {
                *component *= LAVA_DRAG;
            }
            self.motion[1] -= LIQUID_GRAVITY;
            self.leave_liquid_at_bank(world, start_y);
        } else {
            self.travel_on_land(world, strafe, forward);
        }
    }

    /// Hops out of a liquid when blocked sideways with free, dry space a little above.
    fn leave_liquid_at_bank(&mut self, world: &impl World, start_y: f64) {
        if !self.collided_horizontally {
            return;
        }
        let rise = self.motion[1] + f64::from(LIQUID_EXIT_RISE) - self.y + start_y;
        let target = self.bounds.offset_by(self.motion[0], rise, self.motion[2]);
        if self.colliding_boxes(world, &target).is_empty() && !fluid::any_liquid(world, &target) {
            self.motion[1] = f64::from(LIQUID_EXIT_VELOCITY);
        }
    }

    /// Ground and air movement with ladders and gravity.
    fn travel_on_land(&mut self, world: &impl World, strafe: f32, forward: f32) {
        let mut friction = self.surface_friction(world);
        let ground_acceleration = GROUND_ACCELERATION / (friction * friction * friction);
        let acceleration = if self.on_ground {
            self.speed.value() as f32 * ground_acceleration
        } else {
            self.air_speed
        };
        self.accelerate(strafe, forward, acceleration);
        friction = self.surface_friction(world);
        if self.on_ladder(world) {
            let limit = f64::from(LADDER_MAX_SPEED);
            self.motion[0] = clamp_ladder(self.motion[0], limit);
            self.motion[2] = clamp_ladder(self.motion[2], limit);
            self.fall_distance = 0.0;
            if self.motion[1] < LADDER_MAX_FALL {
                self.motion[1] = LADDER_MAX_FALL;
            }
            if self.keys.sneak && self.motion[1] < 0.0 {
                self.motion[1] = 0.0;
            }
        }
        self.move_by(world, self.motion);
        if self.collided_horizontally && self.on_ladder(world) {
            self.motion[1] = LADDER_CLIMB_VELOCITY;
        }
        // The loaded check truncates the position toward zero rather than flooring it.
        if world.block_exists(self.x as i32, 0, self.z as i32) {
            self.motion[1] -= GRAVITY;
        } else {
            self.motion[1] = if self.y > 0.0 { UNLOADED_FALL } else { 0.0 };
        }
        self.motion[1] *= f64::from(VERTICAL_DRAG);
        self.motion[0] *= f64::from(friction);
        self.motion[2] *= f64::from(friction);
    }

    /// Horizontal friction for this tick: the block under the feet when grounded, air otherwise.
    fn surface_friction(&self, world: &impl World) -> f32 {
        if !self.on_ground {
            return AIR_FRICTION;
        }
        let below = world.block(
            floor(self.x),
            floor(self.bounds.min_y).wrapping_sub(1),
            floor(self.z),
        );
        facts(below.id).slipperiness * AIR_FRICTION
    }

    /// Whether the block at the feet is a ladder or vine.
    fn on_ladder(&self, world: &impl World) -> bool {
        let id = world
            .block(floor(self.x), floor(self.bounds.min_y), floor(self.z))
            .id;
        id == block::id::LADDER || id == block::id::VINE
    }

    /// Adds input acceleration rotated by the view yaw; input shorter than 0.01 is ignored and
    /// input longer than 1 is normalised.
    fn accelerate(&mut self, mut strafe: f32, mut forward: f32, acceleration: f32) {
        let mut length = strafe * strafe + forward * forward;
        if length < 1.0e-4 {
            return;
        }
        length = f64::from(length).sqrt() as f32;
        if length < 1.0 {
            length = 1.0;
        }
        let scale = acceleration / length;
        strafe *= scale;
        forward *= scale;
        let sine = sin(self.yaw * std::f32::consts::PI / 180.0);
        let cosine = cos(self.yaw * std::f32::consts::PI / 180.0);
        self.motion[0] += f64::from(strafe * cosine - forward * sine);
        self.motion[2] += f64::from(forward * cosine + strafe * sine);
    }
}

/// Limits a horizontal motion component on a ladder.
fn clamp_ladder(value: f64, limit: f64) -> f64 {
    if value < -limit {
        -limit
    } else if value > limit {
        limit
    } else {
        value
    }
}
