//! Server WIT 0.12's actor motion: the one velocity or knockback a result gives its actor.

use super::*;
use crate::protocol::Position;

const STRANGER: &str = "00000000-0000-4000-8000-000000000001";

fn at(x: f64, y: f64, z: f64) -> Position {
    Position { x, y, z }
}

/// A velocity and a knockback each stage one op for the actor, and a later one of either replaces
/// the earlier, so a result holds at most one motion, in the place of the first.
#[test]
fn motion_stages_one_op_that_a_later_motion_replaces() {
    let mut res = Fixture::new().res();
    assert_eq!(
        res.set_velocity(ACTOR.to_owned(), at(0.25, 0.5, -0.25))
            .unwrap(),
        Ok(())
    );
    assert_eq!(
        res.tell(ACTOR.to_owned(), "moved".to_owned()).unwrap(),
        Ok(())
    );
    assert_eq!(
        res.ops[0],
        Op::SetVelocity {
            player: ACTOR.to_owned(),
            velocity: at(0.25, 0.5, -0.25),
        }
    );
    assert_eq!(
        res.knock_back(ACTOR.to_owned(), at(1.0, 64.0, 2.0), 0.4, 0.3)
            .unwrap(),
        Ok(())
    );
    assert_eq!(
        res.ops,
        vec![
            Op::KnockBack {
                player: ACTOR.to_owned(),
                source: at(1.0, 64.0, 2.0),
                force: 0.4,
                height: 0.3,
            },
            Op::Tell {
                player: ACTOR.to_owned(),
                text: "moved".to_owned(),
            },
        ]
    );
    assert_eq!(
        res.set_velocity(ACTOR.to_owned(), at(0.0, 0.0, 0.0))
            .unwrap(),
        Ok(())
    );
    assert_eq!(
        res.ops[0],
        Op::SetVelocity {
            player: ACTOR.to_owned(),
            velocity: at(0.0, 0.0, 0.0),
        }
    );
    assert_eq!(res.ops.len(), 2);
}

/// Only the actor can be moved: another player is denied, and a callback without an actor has
/// no player to move. Neither stages anything.
#[test]
fn motion_moves_only_the_actor() {
    let mut res = Fixture::new().res();
    assert_eq!(
        res.set_velocity(STRANGER.to_owned(), at(0.0, 1.0, 0.0))
            .unwrap(),
        Err(WorldError::Denied)
    );
    assert_eq!(
        res.knock_back(STRANGER.to_owned(), at(0.0, 64.0, 0.0), 0.4, 0.4)
            .unwrap(),
        Err(WorldError::Denied)
    );
    let mut alone = Fixture {
        actor: None,
        inventory: None,
        ..Fixture::new()
    }
    .res();
    assert_eq!(
        alone
            .set_velocity(ACTOR.to_owned(), at(0.0, 1.0, 0.0))
            .unwrap(),
        Err(WorldError::PlayerUnavailable)
    );
    assert!(res.ops.is_empty() && alone.ops.is_empty());
}

/// A component that is not finite is out of bounds and stages nothing; large finite values are
/// passed on, as the server's own motion packet carries them.
#[test]
fn motion_needs_finite_values() {
    let mut res = Fixture::new().res();
    for velocity in [
        at(f64::NAN, 0.0, 0.0),
        at(0.0, f64::INFINITY, 0.0),
        at(0.0, 0.0, f64::NEG_INFINITY),
    ] {
        assert_eq!(
            res.set_velocity(ACTOR.to_owned(), velocity).unwrap(),
            Err(WorldError::OutOfBounds),
            "{velocity:?}"
        );
    }
    for (source, force, height) in [
        (at(f64::NAN, 0.0, 0.0), 0.4, 0.4),
        (at(0.0, 0.0, 0.0), f64::INFINITY, 0.4),
        (at(0.0, 0.0, 0.0), 0.4, f64::NAN),
    ] {
        assert_eq!(
            res.knock_back(ACTOR.to_owned(), source, force, height)
                .unwrap(),
            Err(WorldError::OutOfBounds),
            "{source:?} {force} {height}"
        );
    }
    assert!(res.ops.is_empty());
    assert_eq!(
        res.set_velocity(ACTOR.to_owned(), at(0.0, 1.0e6, 0.0))
            .unwrap(),
        Ok(())
    );
}
