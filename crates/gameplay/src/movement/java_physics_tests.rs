//! On a Java Edition session the local player follows 1.7.10 movement: the controller's
//! ticks must be the ones `java-sim` computes for the same keys.

use java_sim::player::{Input, Player, Status};
use sim::{Aabb, CollisionQuery, CollisionWorld, MovementInput, WorldQueryError};

use super::{ModeIntent, VersionedFloor, grounded_controller, step};

/// Network hashes of the Bedrock states the core translates 1.7.10 stone and air to.
const STONE: u32 = 2_150_698_529;
const AIR: u32 = 3_690_217_760;

/// The fixture floor, whose top is Y 1, as translated blocks: stone below Y 1.
struct TranslatedFloor;

impl CollisionWorld for TranslatedFloor {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        VersionedFloor(1).collision_boxes(query)
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<sim::BlockPhysicsSample, WorldQueryError> {
        VersionedFloor(1).block_physics(block)
    }

    fn primary_block(&self, block: [i32; 3]) -> Result<Option<u32>, WorldQueryError> {
        Ok(Some(if block[1] == 0 { STONE } else { AIR }))
    }
}

impl java_sim::World for TranslatedFloor {
    fn block(&self, _x: i32, y: i32, _z: i32) -> java_sim::Block {
        if y == 0 {
            java_sim::Block::new(1, 0)
        } else {
            java_sim::Block::AIR
        }
    }

    fn chunk_loaded(&self, _chunk_x: i32, _chunk_z: i32) -> bool {
        true
    }
}

/// The controller's feet: the centre of its box at its base.
fn feet(physics: &super::LocalPhysicsController) -> [f64; 3] {
    let body = physics.body_box().expect("a simulated player");
    [
        (body.min.x + body.max.x) / 2.0,
        body.min.y,
        (body.min.z + body.max.z) / 2.0,
    ]
}

/// Walks forward, sprint-jumps and coasts; the same keys go to the controller and to a
/// `java-sim` player that starts where the controller's player stands.
#[test]
fn a_java_session_moves_the_player_as_java_sim_does() {
    let mut physics = grounded_controller();
    physics.set_java_physics(true);
    assert_eq!(
        physics.sneak_profile(),
        sim::SneakProfile::Legacy,
        "1.7.10 keeps the standing box while sneaking"
    );
    // The anchor arrives in single precision, a hair inside the floor; both players are
    // lifted onto it.
    let [x, sunk, z] = feet(&physics);
    assert!(
        sunk < 1.0 && 1.0 - sunk < 1.0e-5,
        "the fixture anchors the feet at {sunk}"
    );
    let mut reference = Player::at_feet(x, sunk, z, 0.0, 0.0);
    reference.lift_onto_surface(&TranslatedFloor, 1.0e-3);
    let script = [
        (10, true, false, false),
        (12, true, true, true),
        (10, false, false, false),
    ];
    let mut tick = 0;
    for (ticks, forward, sprint, jump) in script {
        for _ in 0..ticks {
            let input = MovementInput {
                forward: f64::from(u8::from(forward)),
                sprinting: sprint,
                jumping: jump,
                ..MovementInput::default()
            };
            let sample = step(&mut physics, input, ModeIntent::default(), &TranslatedFloor);
            let keys = Input {
                forward,
                sprint,
                jump,
                ..Input::default()
            };
            reference.tick(&TranslatedFloor, &keys, &Status::default(), &mut Vec::new());
            assert_eq!(
                feet(&physics).map(f64::to_bits),
                reference.feet().map(f64::to_bits),
                "tick {tick}"
            );
            assert_eq!(
                sample.grounded_after_tick,
                reference.on_ground(),
                "tick {tick} ground"
            );
            assert_eq!(
                sample.vertical_collision,
                reference.on_ground(),
                "tick {tick} reports standing as the vertical collision"
            );
            tick += 1;
        }
    }
    let [_, _, travelled] = reference.feet();
    assert!(
        travelled > 4.0,
        "the script moved the player only {travelled} blocks"
    );
}

/// Without the Java flag the same world and keys follow Bedrock movement, which differs.
#[test]
fn a_bedrock_session_keeps_bedrock_movement() {
    let mut physics = grounded_controller();
    let mut reference = Player::at_feet(0.0, 1.0, 0.0, 0.0, 0.0);
    let mut differed = false;
    for _ in 0..20 {
        let input = MovementInput {
            forward: 1.0,
            sprinting: true,
            jumping: true,
            ..MovementInput::default()
        };
        step(&mut physics, input, ModeIntent::default(), &TranslatedFloor);
        let keys = Input {
            forward: true,
            sprint: true,
            jump: true,
            ..Input::default()
        };
        reference.tick(&TranslatedFloor, &keys, &Status::default(), &mut Vec::new());
        differed |= feet(&physics).map(f64::to_bits) != reference.feet().map(f64::to_bits);
    }
    assert!(differed, "Bedrock movement matched 1.7.10 movement exactly");
}
