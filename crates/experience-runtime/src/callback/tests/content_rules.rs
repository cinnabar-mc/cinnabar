//! Server WIT 0.12 in callbacks: no stack of a block without an item, whether a break harvests,
//! and passing an interaction's click on.

use super::*;
use crate::protocol::{Cause, Change};

/// A block without an item makes no stack: neither a slot nor a drop takes one, and nothing is
/// staged.
#[test]
fn a_block_without_an_item_makes_no_stack() {
    let mut res = Fixture::new().res();
    let hidden = new_stack(HIDDEN, 1, None);
    assert_eq!(
        res.set_slot(0, Some(hidden.clone())).unwrap(),
        Err(WorldError::UnknownBlock)
    );
    assert_eq!(
        res.drop_item(UP, hidden).unwrap(),
        Err(WorldError::UnknownBlock)
    );
    assert!(res.ops.is_empty());
    // A block with an item still makes stacks.
    assert_eq!(
        res.drop_item(UP, new_stack(COUNTER, 1, None)).unwrap(),
        Ok(())
    );
}

/// The actor's break of the counter at [`ANCHOR`], a harvest when `harvest`.
fn broken(harvest: bool) -> CallbackRes {
    let request = Request::Callback {
        seq: 1,
        info: Info {
            context: None,
            simulation: None,
            world_id: "world".to_owned(),
            dimension_id: "overworld".to_owned(),
            tick: 1,
            event_sequence: 1,
        },
        actor: Some(ACTOR.to_owned()),
        world_min_y: -64,
        world_max_y: 319,
        data_budget: 0,
        snapshot: vec![loaded(ANCHOR, AIR, false, None)],
        network: None,
        inventory: None,
        containers: None,
        call: Call::Break {
            change: Change {
                pos: ANCHOR,
                actor: Some(ACTOR.to_owned()),
                cause: Cause::Player,
                before_id: COUNTER.to_owned(),
                after_id: AIR.to_owned(),
                previous_data: None,
            },
            harvest,
        },
    };
    let own = Catalog {
        blocks: [(COUNTER.to_owned(), Vec::new())].into(),
        ..Catalog::default()
    };
    prepare(&Arc::new(own), &request).unwrap().0
}

/// `harvested` holds only in a break that the adapter marks as a harvest.
#[test]
fn only_a_harvesting_break_is_harvested() {
    assert!(!Fixture::new().res().harvested().unwrap());
    for harvest in [false, true] {
        assert_eq!(broken(harvest).harvested().unwrap(), harvest);
    }
}

/// An interaction passes its click on with one staged op, however often it asks; any other
/// callback is denied and stages nothing.
#[test]
fn only_an_interaction_passes_its_click_on() {
    let mut res = Fixture::new().res();
    assert_eq!(res.pass_interaction().unwrap(), Ok(()));
    assert_eq!(res.pass_interaction().unwrap(), Ok(()));
    assert_eq!(res.ops, vec![Op::PassInteraction {}]);
    let mut res = broken(true);
    assert_eq!(res.pass_interaction().unwrap(), Err(WorldError::Denied));
    assert!(res.ops.is_empty());
}
