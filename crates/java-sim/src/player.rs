//! The local player's tick, in the order the 1.7.10 client runs it.

mod attribute;
mod collide;
mod fluid;
mod report;
mod travel;

pub use attribute::{Modifier, MovementSpeed, Operation, SPRINT_MODIFIER_ID};
pub use report::Packet;

use crate::math::{Aabb, floor};
use crate::world::World;

/// Player box width and height, and the eye offset the game keeps between `y` and the feet.
pub const WIDTH: f32 = 0.6;
pub const HEIGHT: f32 = 1.8;
pub const EYE_OFFSET: f32 = 1.62;
/// Largest ledge the player walks up without jumping.
const STEP_HEIGHT: f32 = 0.5;
/// Ticks a second forward tap is accepted as a sprint request, and a second jump as a flight toggle.
const DOUBLE_TAP_TICKS: i32 = 7;
/// Ticks a sprint lasts before the client ends it.
const SPRINT_TICKS: i32 = 600;
/// Forward input a sprint needs.
const SPRINT_FORWARD_THRESHOLD: f32 = 0.8;
/// Input scale while an item is in use.
const ITEM_USE_INPUT_SCALE: f32 = 0.2;
/// Air acceleration, and its sprinting bonus.
const AIR_SPEED: f32 = 0.02;
/// Vertical speed change per tick from the fly up and down keys.
const FLY_VERTICAL_ACCELERATION: f64 = 0.15;
/// Speed the client moves out of a solid block it stands in.
const PUSH_OUT_SPEED: f32 = 0.1;

/// The keys and view angles sampled for one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
    /// View angles in degrees, as accumulated from mouse movement before the tick.
    pub yaw: f32,
    pub pitch: f32,
}

/// Potion effect ids the movement simulation reads.
pub const EFFECT_JUMP_BOOST: u8 = 8;
pub const EFFECT_BLINDNESS: u8 = 15;

/// The potion effects that change movement, held as the client holds them: an effect stays
/// until the server removes it, and applying one that is already active keeps the higher
/// amplifier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    jump_boost: Option<i8>,
    blindness: Option<i8>,
}

impl Effects {
    /// Applies a server effect; ids the simulation does not read are ignored.
    pub fn apply(&mut self, id: u8, amplifier: i8) {
        if let Some(slot) = self.slot(id) {
            *slot = Some(slot.map_or(amplifier, |held| held.max(amplifier)));
        }
    }

    /// Removes an effect the server ended.
    pub fn remove(&mut self, id: u8) {
        if let Some(slot) = self.slot(id) {
            *slot = None;
        }
    }

    fn slot(&mut self, id: u8) -> Option<&mut Option<i8>> {
        match id {
            EFFECT_JUMP_BOOST => Some(&mut self.jump_boost),
            EFFECT_BLINDNESS => Some(&mut self.blindness),
            _ => None,
        }
    }
}

/// Player state the server or other client systems own, read each tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Status {
    /// Whether an item is in use (eating, blocking, drawing a bow).
    pub using_item: bool,
    pub food_level: i32,
    pub effects: Effects,
    /// Whether the player is dead or asleep, which blocks movement input.
    pub movement_blocked: bool,
    /// The sprint state when the caller decides it. `None` leaves sprinting to the game's own
    /// rules: the sprint key, the double tap and the conditions that end a sprint.
    pub sprint_override: Option<bool>,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            using_item: false,
            food_level: 20,
            effects: Effects::default(),
            movement_blocked: false,
            sprint_override: None,
        }
    }
}

/// The abilities the server grants and the flight state the client toggles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Abilities {
    pub invulnerable: bool,
    pub flying: bool,
    pub allow_flying: bool,
    pub creative: bool,
    pub fly_speed: f32,
    pub walk_speed: f32,
}

impl Default for Abilities {
    fn default() -> Self {
        Self {
            invulnerable: false,
            flying: false,
            allow_flying: false,
            creative: false,
            fly_speed: 0.05,
            walk_speed: 0.1,
        }
    }
}

/// Movement keys as the game holds them between ticks: the item-use scale applied in one tick
/// is still visible to the next tick's sprint check.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct MovementKeys {
    strafe: f32,
    forward: f32,
    jump: bool,
    sneak: bool,
}

/// The last position and view the client reported, which decide the next packet.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Reported {
    x: f64,
    min_y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
    ticks_since_position: i32,
    sprinting: bool,
    sneaking: bool,
}

/// The local player.
#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    x: f64,
    /// The game's `posY`: the feet plus the eye offset, less the sneak and step sink.
    y: f64,
    z: f64,
    bounds: Aabb,
    /// Camera sink from sneaking and stepping; it shifts `y` but not the box.
    sink: f32,
    motion: [f64; 3],
    yaw: f32,
    pitch: f32,
    on_ground: bool,
    collided_horizontally: bool,
    collided_vertically: bool,
    in_water: bool,
    in_web: bool,
    fall_distance: f32,
    jump_ticks: i32,
    move_strafe: f32,
    move_forward: f32,
    jumping: bool,
    air_speed: f32,
    sprinting: bool,
    sprint_toggle_timer: i32,
    sprint_ticks_left: i32,
    fly_toggle_timer: i32,
    keys: MovementKeys,
    abilities: Abilities,
    speed: MovementSpeed,
    reported: Reported,
}

impl Player {
    /// A player placed as a server position packet places it: `y` is the packet's Y, the eye
    /// height, and the feet are the eye offset below it.
    #[must_use]
    pub fn new(x: f64, y: f64, z: f64, yaw: f32, pitch: f32) -> Self {
        let mut player = Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            bounds: Aabb::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            sink: 0.0,
            motion: [0.0; 3],
            yaw,
            pitch,
            on_ground: false,
            collided_horizontally: false,
            collided_vertically: false,
            in_water: false,
            in_web: false,
            fall_distance: 0.0,
            jump_ticks: 0,
            move_strafe: 0.0,
            move_forward: 0.0,
            jumping: false,
            air_speed: AIR_SPEED,
            sprinting: false,
            sprint_toggle_timer: 0,
            sprint_ticks_left: 0,
            fly_toggle_timer: 0,
            keys: MovementKeys::default(),
            abilities: Abilities::default(),
            speed: MovementSpeed::default(),
            reported: Reported::default(),
        };
        player.set_position(x, y, z);
        player
    }

    /// A player standing with its feet at `(x, feet_y, z)`.
    #[must_use]
    pub fn at_feet(x: f64, feet_y: f64, z: f64, yaw: f32, pitch: f32) -> Self {
        Self::new(x, feet_y + f64::from(EYE_OFFSET), z, yaw, pitch)
    }

    /// Moves the player to the game's `(x, posY, z)` and rebuilds the box around it.
    fn set_position(&mut self, x: f64, y: f64, z: f64) {
        self.x = x;
        self.y = y;
        self.z = z;
        let half_width = WIDTH / 2.0;
        let min_y = y - f64::from(EYE_OFFSET) + f64::from(self.sink);
        self.bounds = Aabb::new(
            x - f64::from(half_width),
            min_y,
            z - f64::from(half_width),
            x + f64::from(half_width),
            min_y + f64::from(HEIGHT),
            z + f64::from(half_width),
        );
    }

    /// Applies a server position and look: motion stops, the camera sink resets, and the reply
    /// the client sends at once is returned. `y` is the packet's Y, which is the eye height.
    /// The player keeps each angle's remainder of a full turn, while the reply echoes the
    /// packet's angles as sent.
    pub fn teleport(
        &mut self,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
        on_ground: bool,
    ) -> Packet {
        self.sink = 0.0;
        self.motion = [0.0; 3];
        self.yaw = yaw % 360.0;
        self.pitch = pitch % 360.0;
        self.set_position(x, y, z);
        Packet::PositionLook {
            x: self.x,
            y: self.bounds.min_y,
            stance: self.y,
            z: self.z,
            yaw,
            pitch,
            on_ground,
        }
    }

    /// Replaces the motion with a server velocity, in blocks per tick. A component that is
    /// not a number becomes zero: it could never move the player and would never decay.
    pub fn set_velocity(&mut self, x: f64, y: f64, z: f64) {
        self.motion = [x, y, z].map(|component| if component.is_nan() { 0.0 } else { component });
    }

    /// Replaces the motion with the velocity of an entity velocity packet, whose components
    /// are 1/8000 of a block per tick.
    pub fn set_packet_velocity(&mut self, x: i16, y: i16, z: i16) {
        self.set_velocity(
            f64::from(x) / 8000.0,
            f64::from(y) / 8000.0,
            f64::from(z) / 8000.0,
        );
    }

    /// Replaces the abilities with the server's.
    pub fn set_abilities(&mut self, abilities: Abilities) {
        self.abilities = abilities;
    }

    /// The movement speed attribute, for applying server snapshots.
    pub fn movement_speed_mut(&mut self) -> &mut MovementSpeed {
        &mut self.speed
    }

    /// Position as `(x, feet, z)`.
    #[must_use]
    pub fn feet(&self) -> [f64; 3] {
        [self.x, self.bounds.min_y, self.z]
    }

    /// The game's `posY`, which the client reports as the stance.
    #[must_use]
    pub fn stance(&self) -> f64 {
        self.y
    }

    #[must_use]
    pub fn bounds(&self) -> Aabb {
        self.bounds
    }

    #[must_use]
    pub fn motion(&self) -> [f64; 3] {
        self.motion
    }

    #[must_use]
    pub fn view(&self) -> (f32, f32) {
        (self.yaw, self.pitch)
    }

    #[must_use]
    pub fn on_ground(&self) -> bool {
        self.on_ground
    }

    #[must_use]
    pub fn collided_horizontally(&self) -> bool {
        self.collided_horizontally
    }

    #[must_use]
    pub fn collided_vertically(&self) -> bool {
        self.collided_vertically
    }

    #[must_use]
    pub fn sprinting(&self) -> bool {
        self.sprinting
    }

    /// Whether the sneak key was held on the last tick.
    #[must_use]
    pub fn sneaking(&self) -> bool {
        self.keys.sneak
    }

    #[must_use]
    pub fn in_water(&self) -> bool {
        self.in_water
    }

    #[must_use]
    pub fn abilities(&self) -> Abilities {
        self.abilities
    }

    #[must_use]
    pub fn fall_distance(&self) -> f32 {
        self.fall_distance
    }

    /// The camera sink below the standing eye height.
    #[must_use]
    pub fn sink(&self) -> f32 {
        self.sink
    }

    /// Runs one client tick and appends the packets the client sends for it to `packets`.
    ///
    /// Nothing happens, and nothing is sent, while the player's column is not loaded.
    pub fn tick(
        &mut self,
        world: &impl World,
        input: &Input,
        status: &Status,
        packets: &mut Vec<Packet>,
    ) {
        self.yaw = input.yaw;
        self.pitch = input.pitch;
        if !world.block_exists(floor(self.x), 0, floor(self.z)) {
            return;
        }
        self.update_liquid_state(world);
        self.living_update(world, input, status, packets);
        self.report_motion(packets);
    }

    /// Start-of-tick liquid handling: water pushes the player and clears its fall; lava halves it.
    fn update_liquid_state(&mut self, world: &impl World) {
        self.handle_water_movement(world);
        if fluid::touches_lava(world, &self.bounds) {
            self.fall_distance *= 0.5;
        }
    }

    /// Input, sprint and flight handling, then the move itself.
    fn living_update(
        &mut self,
        world: &impl World,
        input: &Input,
        status: &Status,
        packets: &mut Vec<Packet>,
    ) {
        if self.sprint_ticks_left > 0 {
            self.sprint_ticks_left -= 1;
            if self.sprint_ticks_left == 0 {
                self.set_sprinting(false);
            }
        }
        if self.sprint_toggle_timer > 0 {
            self.sprint_toggle_timer -= 1;
        }
        let was_jumping = self.keys.jump;
        let was_forward = self.keys.forward >= SPRINT_FORWARD_THRESHOLD;
        self.keys = sample_keys(input);
        if status.using_item {
            self.keys.strafe *= ITEM_USE_INPUT_SCALE;
            self.keys.forward *= ITEM_USE_INPUT_SCALE;
            self.sprint_toggle_timer = 0;
        }
        if self.keys.sneak && self.sink < 0.2 {
            self.sink = 0.2;
        }
        self.push_out_of_blocks(world);

        let fed = status.food_level as f32 > 6.0 || self.abilities.allow_flying;
        let forward = self.keys.forward >= SPRINT_FORWARD_THRESHOLD;
        if let Some(sprinting) = status.sprint_override {
            if self.sprinting != sprinting {
                self.set_sprinting(sprinting);
            }
        } else {
            if self.on_ground
                && !was_forward
                && forward
                && !self.sprinting
                && fed
                && !status.using_item
                && !status.effects.blindness.is_some()
            {
                if self.sprint_toggle_timer > 0 || input.sprint {
                    self.set_sprinting(true);
                } else {
                    self.sprint_toggle_timer = DOUBLE_TAP_TICKS;
                }
            }
            if !self.sprinting
                && forward
                && fed
                && !status.using_item
                && !status.effects.blindness.is_some()
                && input.sprint
            {
                self.set_sprinting(true);
            }
            if self.sprinting && (!forward || self.collided_horizontally || !fed) {
                self.set_sprinting(false);
            }
        }
        if self.abilities.allow_flying && !was_jumping && self.keys.jump {
            if self.fly_toggle_timer == 0 {
                self.fly_toggle_timer = DOUBLE_TAP_TICKS;
            } else {
                self.abilities.flying = !self.abilities.flying;
                packets.push(Packet::abilities(&self.abilities));
                self.fly_toggle_timer = 0;
            }
        }
        if self.abilities.flying {
            if self.keys.sneak {
                self.motion[1] -= FLY_VERTICAL_ACCELERATION;
            }
            if self.keys.jump {
                self.motion[1] += FLY_VERTICAL_ACCELERATION;
            }
        }

        if self.fly_toggle_timer > 0 {
            self.fly_toggle_timer -= 1;
        }
        self.travel_tick(world, status);
        self.air_speed = AIR_SPEED;
        if self.sprinting {
            self.air_speed = (f64::from(self.air_speed) + f64::from(AIR_SPEED) * 0.3) as f32;
        }

        if self.on_ground && self.abilities.flying {
            self.abilities.flying = false;
            packets.push(Packet::abilities(&self.abilities));
        }
    }

    /// Changes the sprint state and the speed modifier that goes with it.
    fn set_sprinting(&mut self, sprinting: bool) {
        self.sprinting = sprinting;
        self.speed.set_sprinting(sprinting);
        self.sprint_ticks_left = if sprinting { SPRINT_TICKS } else { 0 };
    }

    /// Nudges the player sideways out of full solid blocks at four points inside its box.
    fn push_out_of_blocks(&mut self, world: &impl World) {
        let reach = f64::from(WIDTH) * 0.35;
        let y = self.bounds.min_y + 0.5;
        self.push_out_of_block(world, self.x - reach, y, self.z + reach);
        self.push_out_of_block(world, self.x - reach, y, self.z - reach);
        self.push_out_of_block(world, self.x + reach, y, self.z - reach);
        self.push_out_of_block(world, self.x + reach, y, self.z + reach);
    }

    /// When the point is inside a full solid block, sets horizontal motion toward the nearest
    /// side that has two free blocks.
    fn push_out_of_block(&mut self, world: &impl World, x: f64, y: f64, z: f64) {
        let (block_x, block_y, block_z) = (floor(x), floor(y), floor(z));
        let offset_x = x - f64::from(block_x);
        let offset_z = z - f64::from(block_z);
        let above = block_y.wrapping_add(1);
        let normal =
            |x: i32, y: i32, z: i32| crate::block::facts(world.block(x, y, z).id).normal_cube;
        if !normal(block_x, block_y, block_z) && !normal(block_x, above, block_z) {
            return;
        }
        let free = |x: i32, z: i32| !normal(x, block_y, z) && !normal(x, above, z);
        let mut nearest = 9999.0;
        let mut side = None;
        if free(block_x.wrapping_sub(1), block_z) && offset_x < nearest {
            nearest = offset_x;
            side = Some(0);
        }
        if free(block_x.wrapping_add(1), block_z) && 1.0 - offset_x < nearest {
            nearest = 1.0 - offset_x;
            side = Some(1);
        }
        if free(block_x, block_z.wrapping_sub(1)) && offset_z < nearest {
            nearest = offset_z;
            side = Some(4);
        }
        if free(block_x, block_z.wrapping_add(1)) && 1.0 - offset_z < nearest {
            side = Some(5);
        }
        match side {
            Some(0) => self.motion[0] = f64::from(-PUSH_OUT_SPEED),
            Some(1) => self.motion[0] = f64::from(PUSH_OUT_SPEED),
            Some(4) => self.motion[2] = f64::from(-PUSH_OUT_SPEED),
            Some(5) => self.motion[2] = f64::from(PUSH_OUT_SPEED),
            _ => {}
        }
    }
}

/// Converts held keys to movement input; sneaking scales both axes.
fn sample_keys(input: &Input) -> MovementKeys {
    let mut keys = MovementKeys {
        strafe: 0.0,
        forward: 0.0,
        jump: input.jump,
        sneak: input.sneak,
    };
    if input.forward {
        keys.forward += 1.0;
    }
    if input.back {
        keys.forward -= 1.0;
    }
    if input.left {
        keys.strafe += 1.0;
    }
    if input.right {
        keys.strafe -= 1.0;
    }
    if keys.sneak {
        keys.strafe = (f64::from(keys.strafe) * 0.3) as f32;
        keys.forward = (f64::from(keys.forward) * 0.3) as f32;
    }
    keys
}
