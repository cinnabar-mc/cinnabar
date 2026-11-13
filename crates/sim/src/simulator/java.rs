//! Java Edition 1.7.10 physics for sessions the core translates from a Java server.
//!
//! The tick runs `java-sim`'s player behind the Bedrock-shaped [`PlayerState`]: the state
//! carries the position, velocity and ground flag the rest of the client reads, and the Java
//! player carries everything else the 1.7.10 client remembers between ticks. Blocks are read
//! by the 1.7.10 id the server sent, recovered from each block's Bedrock state.

use std::cell::Cell;

use java_sim::player::{Abilities, Effects, Input, Player, Status};
use java_sim::{Block, WORLD_HEIGHT, World};

use super::{
    AxisCollisions, ControlledTickResult, MovementEnvironment, MovementInput, MovementMode,
    PlayerState, SimulationError, TickResult, controls,
};
use crate::{CollisionWorld, Vec3, WorldCollisionIdentity, WorldQueryError};

/// Jump Boost and Blindness ids of the 1.7.10 effect list.
const JUMP_BOOST: u8 = java_sim::player::EFFECT_JUMP_BOOST;
const BLINDNESS: u8 = java_sim::player::EFFECT_BLINDNESS;
/// 1.7.10 piston ids and metadata bits: the low three bits face the piston and bit 8 marks
/// an extended base.
const STICKY_PISTON: u16 = 29;
const PISTON: u16 = 33;
const PISTON_HEAD: u16 = 34;
const PISTON_FACING: u8 = 7;
const PISTON_EXTENDED: u8 = 8;
/// Block offsets of the six facings: down, up, north, south, west, east.
const FACING_OFFSETS: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];
/// How far a server position can sink the feet into the ground: positions reach the client
/// in single precision, whose error at world coordinates stays well below this.
const PLACEMENT_TOLERANCE: f64 = 1.0e-3;
/// Food level of a player who may sprint; the client's own sprint decision already
/// accounts for hunger.
const FED: i32 = 20;

/// The Java player behind a state, with the position and velocity it last wrote there.
/// A state whose values differ was moved or pushed by the server since.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Shadow {
    player: Player,
    position: Vec3,
    velocity: Vec3,
}

/// A `java-sim` world over the client's collision world. Unloaded and unknown blocks read
/// as air, as they do in the game; the first query error other than an unloaded column is
/// kept for the caller.
struct JavaWorld<'a, W> {
    world: &'a W,
    failure: Cell<Option<WorldQueryError>>,
}

impl<W: CollisionWorld> JavaWorld<'_, W> {
    /// The network id at a position, or `None` where the column is not loaded. The first
    /// query error of any other kind is kept in `failure`.
    fn network_id(&self, x: i32, y: i32, z: i32) -> Option<u32> {
        match self.world.primary_block([x, y, z]) {
            Ok(id) => id,
            Err(WorldQueryError::UnloadedChunk(_)) => None,
            Err(error) => {
                let first = self.failure.take().unwrap_or(error);
                self.failure.set(Some(first));
                None
            }
        }
    }
}

impl<W: CollisionWorld> JavaWorld<'_, W> {
    /// The 1.7.10 block the state at a position translates back to, without its neighbours.
    fn translated(&self, x: i32, y: i32, z: i32) -> Block {
        if !(0..WORLD_HEIGHT).contains(&y) {
            return Block::AIR;
        }
        self.network_id(x, y, z)
            .and_then(assets::facts::java_legacy::java_legacy_block)
            .map_or(Block::AIR, |block| Block::new(block.id, block.metadata))
    }
}

impl<W: CollisionWorld> World for JavaWorld<'_, W> {
    fn block(&self, x: i32, y: i32, z: i32) -> Block {
        let block = self.translated(x, y, z);
        // Bedrock keeps a piston's extension in its block entity, so the state of an extended
        // base is that of a retracted one. The head in front of it tells them apart.
        if matches!(block.id, STICKY_PISTON | PISTON)
            && block.meta & PISTON_EXTENDED == 0
            && let Some([dx, dy, dz]) = FACING_OFFSETS.get(usize::from(block.meta))
        {
            let head = self.translated(
                x.wrapping_add(*dx),
                y.wrapping_add(*dy),
                z.wrapping_add(*dz),
            );
            if head.id == PISTON_HEAD && head.meta & PISTON_FACING == block.meta {
                return Block::new(block.id, block.meta | PISTON_EXTENDED);
            }
        }
        block
    }

    fn chunk_loaded(&self, chunk_x: i32, chunk_z: i32) -> bool {
        self.network_id(chunk_x << 4, 0, chunk_z << 4).is_some()
    }
}

/// Advances one tick of Java Edition 1.7.10 physics.
pub(super) fn tick(
    state: &mut PlayerState,
    input: MovementInput,
    world: &impl CollisionWorld,
) -> Result<ControlledTickResult, SimulationError> {
    super::state::validate(state)?;
    super::input::validate(input)?;
    let tick = state
        .tick
        .checked_add(1)
        .ok_or(SimulationError::TickOverflow)?;
    let (yaw, pitch) = (input.yaw_degrees as f32, input.pitch_degrees as f32);
    let java_world = JavaWorld {
        world,
        failure: Cell::new(None),
    };
    // The state keeps its own player until the tick succeeds, so a failed one changes nothing.
    let mut shadow = match state.java.as_deref() {
        Some(shadow) => shadow.clone(),
        None => {
            let mut player = Player::at_feet(
                state.position.x,
                state.position.y,
                state.position.z,
                yaw,
                pitch,
            );
            player.set_velocity(state.velocity.x, state.velocity.y, state.velocity.z);
            player.lift_onto_surface(&java_world, PLACEMENT_TOLERANCE);
            Shadow {
                player,
                position: state.position,
                velocity: state.velocity,
            }
        }
    };
    if state.position != shadow.position {
        // A server teleport: the 1.7.10 client stops and stands at the new position. The
        // core has already sent the reply the client makes.
        let eye = state.position.y + f64::from(java_sim::player::EYE_OFFSET);
        let _ = shadow
            .player
            .teleport(state.position.x, eye, state.position.z, yaw, pitch, false);
        shadow
            .player
            .lift_onto_surface(&java_world, PLACEMENT_TOLERANCE);
    }
    if state.velocity != shadow.velocity {
        // A server velocity: knockback, an explosion, or the stop a teleport brings.
        shadow
            .player
            .set_velocity(state.velocity.x, state.velocity.y, state.velocity.z);
    }

    let flying = input.mode == MovementMode::Flying;
    // Flight and sprinting start and stop by the client's own rules, which also decide what
    // the server is told, so the Java player never changes either by itself.
    shadow.player.set_abilities(Abilities {
        flying,
        allow_flying: false,
        fly_speed: input
            .fly_speed
            .map_or(Abilities::default().fly_speed, |speed| speed as f32),
        ..Abilities::default()
    });
    let mut effects = Effects::default();
    if let Some(amplifier) = input.effects.jump_boost {
        effects.apply(
            JUMP_BOOST,
            amplifier.clamp(i32::from(i8::MIN), i32::from(i8::MAX)) as i8,
        );
    }
    if input.effects.blindness {
        effects.apply(BLINDNESS, 0);
    }
    let status = Status {
        using_item: input.using_consumable || input.item_use_movement_modifier.is_some(),
        food_level: FED,
        effects,
        movement_blocked: input.immobile,
        sprint_override: Some(input.sprinting),
    };
    let keys = Input {
        forward: input.forward > 0.0,
        back: input.forward < 0.0,
        left: input.strafe > 0.0,
        right: input.strafe < 0.0,
        jump: input.jumping,
        sneak: input.sneaking,
        sprint: input.sprinting,
        yaw,
        pitch,
    };

    let before = shadow.player.feet();
    // The packets belong to the core, which derives the same ones from the reported motion.
    shadow
        .player
        .tick(&java_world, &keys, &status, &mut Vec::new());
    if let Some(error) = java_world.failure.take() {
        return Err(SimulationError::World(error));
    }

    let [x, feet, z] = shadow.player.feet();
    let [motion_x, motion_y, motion_z] = shadow.player.motion();
    let blocked = shadow.player.collided_horizontally();
    state.tick = tick;
    state.position = Vec3::new(x, feet, z);
    state.velocity = Vec3::new(motion_x, motion_y, motion_z);
    state.movement = Vec3::new(x - before[0], feet - before[1], z - before[2]);
    state.requested_movement = state.movement;
    state.on_ground = shadow.player.on_ground();
    // The vertical flag carries the ground state to the core, which has no other source
    // for it: Bedrock input reports collisions, not standing.
    state.collisions = AxisCollisions {
        x: blocked,
        y: state.on_ground,
        z: blocked,
    };
    shadow.position = state.position;
    shadow.velocity = state.velocity;
    let environment = MovementEnvironment {
        in_water: shadow.player.in_water(),
        ..MovementEnvironment::default()
    };
    state.java = Some(Box::new(shadow));
    Ok(ControlledTickResult {
        tick_result: TickResult {
            tick,
            position: state.position,
            velocity: state.velocity,
            movement: state.movement,
            collisions: state.collisions,
            on_ground: state.on_ground,
            environment,
            world_identity: WorldCollisionIdentity {
                registry: world.registry_identity(),
                chunks: Box::default(),
            },
        },
        controls: controls::process(input),
        jump_initiated: false,
    })
}
