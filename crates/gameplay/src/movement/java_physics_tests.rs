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
            assert_eq!(
                sample
                    .processed
                    .java_position
                    .map(|feet| feet.map(f64::to_bits)),
                Some(reference.feet().map(f64::to_bits)),
                "tick {tick} carries the position in double precision"
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
        let sample = step(&mut physics, input, ModeIntent::default(), &TranslatedFloor);
        assert_eq!(sample.processed.java_position, None);
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

/// A server teleport moves the player the 1.7.10 client already has: it still stands on the
/// ground, so a held jump leaves the ground on the next tick. A fresh player would start
/// airborne and jump late.
#[test]
fn a_server_teleport_keeps_the_java_player() {
    let mut physics = grounded_controller();
    physics.set_java_physics(true);
    let [x, sunk, z] = feet(&physics);
    let mut reference = Player::at_feet(x, sunk, z, 0.0, 0.0);
    reference.lift_onto_surface(&TranslatedFloor, 1.0e-3);
    for _ in 0..3 {
        step(
            &mut physics,
            MovementInput::default(),
            ModeIntent::default(),
            &TranslatedFloor,
        );
        reference.tick(
            &TranslatedFloor,
            &Input::default(),
            &Status::default(),
            &mut Vec::new(),
        );
    }
    assert!(reference.on_ground());

    let tick = physics.state().expect("a simulated player").tick;
    physics
        .apply_correction(
            crate::movement::PhysicsAnchor {
                network_position: [5.5, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 0.5],
                tick,
                on_ground: true,
                velocity: None,
            },
            crate::movement::PhysicsCorrectionMode::Snap,
            None,
            &TranslatedFloor,
        )
        .expect("the teleport snaps");
    // The frame the teleport arrives in is not simulated.
    let arrival = physics.advance(super::TICK, MovementInput::default(), &TranslatedFloor);
    assert!(arrival.samples.is_empty(), "the arrival frame ticked");
    let [x, sunk, z] = feet(&physics);
    let eye = sunk + f64::from(java_sim::player::EYE_OFFSET);
    let _ = reference.teleport(x, eye, z, 0.0, 0.0, false);
    reference.lift_onto_surface(&TranslatedFloor, 1.0e-3);

    for tick in 0..6 {
        let input = MovementInput {
            jumping: true,
            ..MovementInput::default()
        };
        let sample = step(&mut physics, input, ModeIntent::default(), &TranslatedFloor);
        let keys = Input {
            jump: true,
            ..Input::default()
        };
        reference.tick(&TranslatedFloor, &keys, &Status::default(), &mut Vec::new());
        assert_eq!(
            feet(&physics).map(f64::to_bits),
            reference.feet().map(f64::to_bits),
            "tick {tick} after the teleport"
        );
        assert_eq!(
            sample.grounded_after_tick,
            reference.on_ground(),
            "tick {tick}"
        );
    }
    let [_, height, _] = reference.feet();
    assert!(height > 1.5, "the held jump only reached Y {height}");
}

/// The client decides sprinting and reports it to the server, so the movement follows that
/// decision: a sprint that ends while forward stays held slows to a walk.
#[test]
fn a_sprint_that_ends_with_forward_held_slows_to_a_walk() {
    let run = |sprint_ticks: usize| {
        let mut physics = grounded_controller();
        physics.set_java_physics(true);
        let mut path = Vec::new();
        for tick in 0..80 {
            let input = MovementInput {
                forward: 1.0,
                sprinting: tick < sprint_ticks,
                ..MovementInput::default()
            };
            step(&mut physics, input, ModeIntent::default(), &TranslatedFloor);
            path.push(feet(&physics)[2]);
        }
        path
    };
    let (stopped, walked) = (run(20), run(0));
    assert!(
        stopped[19] > walked[19] + 0.5,
        "the sprint covered no extra ground"
    );
    let step_of = |path: &[f64]| path[79] - path[78];
    assert!(
        (step_of(&stopped) - step_of(&walked)).abs() < 1.0e-9,
        "the player still moves {} a tick, a walk moves {}",
        step_of(&stopped),
        step_of(&walked)
    );
}

/// A respawn moves the player like a teleport, but the 1.7.10 client replaces its player
/// there: the new one starts in the air, so a held jump leaves the ground a tick later than
/// the old, grounded player would.
#[test]
fn a_respawn_starts_a_new_java_player() {
    use crate::committed_control::CommittedGameplayState;
    use crate::movement::{
        LocalMovementEffectTimeline, LocalMovementSpeedAuthority, MovementSource, MovementTicker,
    };

    let mut physics = grounded_controller();
    physics.set_java_physics(true);
    for _ in 0..3 {
        step(
            &mut physics,
            MovementInput::default(),
            ModeIntent::default(),
            &TranslatedFloor,
        );
    }
    let tick = physics.state().expect("a simulated player").tick;
    let mut movement = MovementTicker::default();
    movement.reset(7, tick, physics.network_position().expect("a position"));
    movement.set_source(MovementSource::Physics);
    let mut effects = LocalMovementEffectTimeline::default();
    effects.begin_session(7);
    let mut speed = LocalMovementSpeedAuthority::default();
    speed.begin_session(7, 0);
    let target = [5.5, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 0.5];
    let resolved = client_world::ResolvedServerPosition {
        position: target,
        surface_anchor: None,
    };
    CommittedGameplayState {
        movement: &mut movement,
        physics: &mut physics,
        effects: &mut effects,
        speed: &mut speed,
        session_generation: 7,
        dimension: 0,
        dimension_transfer_active: false,
    }
    .apply(
        client_world::CommittedControlEvent::Respawn {
            sequence: 1,
            respawn: protocol::RespawnEvent {
                position: target,
                state: 1,
                runtime_entity_id: 0,
            },
            resolved,
        },
        &TranslatedFloor,
        |_| {},
    );
    assert_eq!(
        physics.network_position(),
        Some(target),
        "the respawn applied"
    );
    let arrival = physics.advance(super::TICK, MovementInput::default(), &TranslatedFloor);
    assert!(arrival.samples.is_empty(), "the arrival frame ticked");

    let [x, sunk, z] = feet(&physics);
    let mut fresh = Player::at_feet(x, sunk, z, 0.0, 0.0);
    fresh.lift_onto_surface(&TranslatedFloor, 1.0e-3);
    let mut kept = fresh.clone();
    for _ in 0..3 {
        kept.tick(
            &TranslatedFloor,
            &Input::default(),
            &Status::default(),
            &mut Vec::new(),
        );
    }
    let eye = sunk + f64::from(java_sim::player::EYE_OFFSET);
    let _ = kept.teleport(x, eye, z, 0.0, 0.0, false);
    kept.lift_onto_surface(&TranslatedFloor, 1.0e-3);

    let keys = Input {
        jump: true,
        ..Input::default()
    };
    let mut differed = false;
    for tick in 0..6 {
        let input = MovementInput {
            jumping: true,
            ..MovementInput::default()
        };
        step(&mut physics, input, ModeIntent::default(), &TranslatedFloor);
        fresh.tick(&TranslatedFloor, &keys, &Status::default(), &mut Vec::new());
        kept.tick(&TranslatedFloor, &keys, &Status::default(), &mut Vec::new());
        assert_eq!(
            feet(&physics).map(f64::to_bits),
            fresh.feet().map(f64::to_bits),
            "tick {tick} after the respawn"
        );
        differed |= fresh.feet() != kept.feet();
    }
    assert!(differed, "a kept player would have moved the same way");
}

/// A correction replays the ticks after it, and each replayed tick reports its new
/// position in double precision rather than the one it had before the correction.
#[test]
fn a_replayed_tick_carries_its_new_exact_position() {
    let mut physics = grounded_controller();
    physics.set_java_physics(true);
    let mut samples = Vec::new();
    for _ in 0..6 {
        let input = MovementInput {
            forward: 1.0,
            ..MovementInput::default()
        };
        samples.push(step(
            &mut physics,
            input,
            ModeIntent::default(),
            &TranslatedFloor,
        ));
    }
    let corrected = &samples[2];
    let mut position = corrected.position;
    position[0] += 0.5;
    let plan = physics
        .apply_correction(
            crate::movement::PhysicsAnchor {
                network_position: position,
                tick: corrected.tick,
                on_ground: true,
                velocity: None,
            },
            crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
            None,
            &TranslatedFloor,
        )
        .expect("the correction replays");
    assert!(!plan.replayed_samples.is_empty(), "no tick was replayed");
    for replayed in &plan.replayed_samples {
        let exact = replayed
            .processed
            .java_position
            .expect("a Java tick carries its exact position");
        assert_eq!(
            [exact[0] as f32, exact[2] as f32],
            [replayed.position[0], replayed.position[2]],
            "tick {} reports {exact:?} for {:?}",
            replayed.tick,
            replayed.position
        );
    }
}
