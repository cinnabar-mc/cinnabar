//! A caller that decides sprinting itself: the player sprints exactly when told to, whatever
//! its keys and timers would decide.

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

/// Walks forward for `ticks` and returns the feet after each one. `sprint_key` holds the
/// sprint key; `decided` gives the caller's sprint decision for a tick, if it makes one.
fn walk(ticks: usize, sprint_key: bool, decided: impl Fn(usize) -> Option<bool>) -> Vec<[u64; 3]> {
    let mut player = Player::at_feet(0.5, 64.0, 0.5, 0.0, 0.0);
    let keys = Input {
        forward: true,
        sprint: sprint_key,
        ..Input::default()
    };
    (0..ticks)
        .map(|tick| {
            let status = Status {
                sprint_override: decided(tick),
                ..Status::default()
            };
            player.tick(&Floor, &keys, &status, &mut Vec::new());
            assert_eq!(
                player.sprinting(),
                decided(tick).unwrap_or(sprint_key),
                "tick {tick}"
            );
            player.feet().map(f64::to_bits)
        })
        .collect()
}

#[test]
fn a_decided_sprint_moves_as_the_sprint_key_does() {
    assert_eq!(walk(40, false, |_| Some(true)), walk(40, true, |_| None));
}

#[test]
fn a_refused_sprint_walks_despite_the_sprint_key() {
    assert_eq!(walk(40, true, |_| Some(false)), walk(40, false, |_| None));
}

/// The game ends a sprint only when forward is released or the player runs into something.
/// A caller that ends it while forward stays held slows the player to a walk.
#[test]
fn a_sprint_the_caller_ends_slows_to_a_walk_with_forward_held() {
    let stopped = walk(80, false, |tick| Some(tick < 20));
    let walked = walk(80, false, |_| None);
    let step = |path: &[[u64; 3]]| f64::from_bits(path[79][2]) - f64::from_bits(path[78][2]);
    assert!(
        f64::from_bits(stopped[19][2]) > f64::from_bits(walked[19][2]) + 0.5,
        "the sprint covered no extra ground"
    );
    assert!(
        (step(&stopped) - step(&walked)).abs() < 1.0e-9,
        "the player still moves {} a tick, a walk moves {}",
        step(&stopped),
        step(&walked)
    );
}
