//! Server WIT 0.12's actor damage: the one hurt a result gives its actor.

use super::*;

const STRANGER: &str = "00000000-0000-4000-8000-000000000001";

/// Damage stages one op for the actor, and a later call replaces it in place.
#[test]
fn damage_stages_one_op_that_a_later_damage_replaces() {
    let mut res = Fixture::new().res();
    assert_eq!(res.damage(ACTOR.to_owned(), 4.0, true).unwrap(), Ok(()));
    assert_eq!(
        res.tell(ACTOR.to_owned(), "hurt".to_owned()).unwrap(),
        Ok(())
    );
    assert_eq!(res.damage(ACTOR.to_owned(), 2.5, false).unwrap(), Ok(()));
    assert_eq!(
        res.ops,
        vec![
            Op::Damage {
                player: ACTOR.to_owned(),
                amount: 2.5,
                reduced_by_armour: false,
            },
            Op::Tell {
                player: ACTOR.to_owned(),
                text: "hurt".to_owned(),
            },
        ]
    );
}

/// Only the actor can be damaged: another player is denied, and a callback without an actor
/// has no player to damage. Neither stages anything.
#[test]
fn damage_hurts_only_the_actor() {
    let mut res = Fixture::new().res();
    assert_eq!(
        res.damage(STRANGER.to_owned(), 1.0, true).unwrap(),
        Err(WorldError::Denied)
    );
    let mut alone = Fixture {
        actor: None,
        inventory: None,
        ..Fixture::new()
    }
    .res();
    assert_eq!(
        alone.damage(ACTOR.to_owned(), 1.0, true).unwrap(),
        Err(WorldError::PlayerUnavailable)
    );
    assert!(res.ops.is_empty() && alone.ops.is_empty());
}

/// An amount that is negative or not finite is out of bounds and stages nothing; zero and large
/// finite amounts are passed on.
#[test]
fn damage_needs_a_finite_amount_of_at_least_zero() {
    let mut res = Fixture::new().res();
    for amount in [-0.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            res.damage(ACTOR.to_owned(), amount, true).unwrap(),
            Err(WorldError::OutOfBounds),
            "{amount}"
        );
    }
    assert!(res.ops.is_empty());
    assert_eq!(res.damage(ACTOR.to_owned(), 0.0, true).unwrap(), Ok(()));
    assert_eq!(res.damage(ACTOR.to_owned(), 1.0e6, true).unwrap(), Ok(()));
}
