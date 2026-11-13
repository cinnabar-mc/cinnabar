//! Inputs outside anything a recorded client reached: the simulation must stay bounded and
//! must not panic.

use java_sim::{Block, Input, Player, Status, World};

/// A stone floor under the whole world, with every column loaded.
struct Floor;

impl World for Floor {
    fn block(&self, _x: i32, y: i32, _z: i32) -> Block {
        if y == 63 {
            Block::new(1, 0)
        } else {
            Block::AIR
        }
    }

    fn chunk_loaded(&self, _chunk_x: i32, _chunk_z: i32) -> bool {
        true
    }
}

/// A player that has landed on the floor.
fn grounded() -> Player {
    let mut player = Player::at_feet(0.5, 64.0, 0.5, 0.0, 0.0);
    let mut packets = Vec::new();
    for _ in 0..3 {
        player.tick(&Floor, &Input::default(), &Status::default(), &mut packets);
    }
    assert!(player.on_ground());
    player
}

/// The sneak edge search steps a move back 0.05 at a time. A move too large for that step
/// to change would never end, and a huge move would scan every block it crosses.
#[test]
fn a_huge_server_velocity_moves_the_player_a_bounded_distance() {
    for velocity in [1e16, -1e16, f64::INFINITY, f64::MAX] {
        let mut player = grounded();
        player.set_velocity(velocity, 0.0, velocity);
        let sneak = Input {
            sneak: true,
            ..Input::default()
        };
        player.tick(&Floor, &sneak, &Status::default(), &mut Vec::new());
        let [x, _, z] = player.feet();
        assert!(
            x.abs() <= 101.0 && z.abs() <= 101.0,
            "velocity {velocity} moved the player to {x}, {z}"
        );
    }
}

#[test]
fn a_velocity_that_is_not_a_number_does_not_move_the_player() {
    let mut player = grounded();
    let before = player.feet();
    player.set_velocity(f64::NAN, 0.0, f64::NAN);
    player.tick(
        &Floor,
        &Input::default(),
        &Status::default(),
        &mut Vec::new(),
    );
    let after = player.feet();
    assert_eq!((before[0], before[2]), (after[0], after[2]));
}

/// Coordinates beyond the `i32` range reach block arithmetic through the floor function.
#[test]
fn positions_beyond_the_integer_range_do_not_panic() {
    for coordinate in [-3.0e9, 3.0e9, -2_147_483_649.5, 2_147_483_647.5] {
        let mut player = Player::at_feet(coordinate, 64.0, coordinate, 0.0, 0.0);
        let forward = Input {
            forward: true,
            jump: true,
            ..Input::default()
        };
        for _ in 0..3 {
            player.tick(&Floor, &forward, &Status::default(), &mut Vec::new());
        }
    }
}
