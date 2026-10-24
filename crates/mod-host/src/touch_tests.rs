use super::*;
use crate::ModGrants;
use wit::Host as _;

fn state(controls: bool) -> State {
    State::new(
        ModGrants {
            controls,
            ..Default::default()
        },
        String::new(),
        Default::default(),
    )
}

fn button(id: &str) -> wit::Button {
    wit::Button {
        id: id.into(),
        label: "Menu".into(),
        anchor: hud::Anchor::TopRight,
        offset: hud::Point { x: 4., y: 24. },
        size: hud::Point { x: 36., y: 18. },
    }
}

/// Buttons publish only on commit, convert their placement exactly and replace the whole set.
#[test]
fn buttons_commit_transactionally_and_replace_the_set() {
    let mut state = state(true);
    state.touch.begin_callback();
    state
        .set_buttons(vec![button("demo-menu"), button("demo-toggle")])
        .unwrap()
        .unwrap();
    assert!(state.touch.committed.is_empty());
    state.touch.commit();
    assert_eq!(
        state.touch.committed[0],
        TouchButton {
            id: "demo-menu".into(),
            label: "Menu".into(),
            anchor: ui::mod_hud::Anchor::TopRight,
            offset: [4., 24.],
            size: [36., 18.],
        }
    );
    state.touch.begin_callback();
    state
        .set_buttons(vec![button("demo-toggle")])
        .unwrap()
        .unwrap();
    state.touch.commit();
    assert_eq!(state.touch.committed.len(), 1);
    state.touch.begin_callback();
    state.set_buttons(Vec::new()).unwrap().unwrap();
    state.touch.commit();
    assert!(state.touch.committed.is_empty());
}

/// Authority, validation and the per-callback budget are checked before anything is staged.
#[test]
fn buttons_require_controls_and_valid_bounded_input() {
    let mut denied = state(false);
    assert!(denied.set_buttons(vec![button("a")]).unwrap().is_err());
    assert!(denied.read_presses().unwrap().is_err());

    let mut state = state(true);
    state.touch.begin_callback();
    assert!(
        state
            .set_buttons(vec![button("a"), button("a")])
            .unwrap()
            .is_err()
    );
    let mut wide = button("a");
    wide.size.x = 400.;
    assert!(state.set_buttons(vec![wide]).unwrap().is_err());
    state.touch.commit();
    assert!(state.touch.committed.is_empty());

    state.touch.begin_callback();
    for _ in 0..MAX_IMPORT_WRITES {
        state.set_buttons(Vec::new()).unwrap().unwrap();
    }
    assert!(state.set_buttons(Vec::new()).is_err());
}

/// Taps are visible only to a frame, survive a deferred frame and are consumed by an executed one.
#[test]
fn taps_queue_for_the_next_frame_and_survive_deferral() {
    let mut state = state(true);
    state.touch.begin_callback();
    state
        .set_buttons(vec![button("demo-menu")])
        .unwrap()
        .unwrap();
    state.touch.commit();
    assert!(state.touch.queue("demo-menu"));
    assert!(!state.touch.queue("other"));

    state.touch.begin_callback();
    assert!(state.read_presses().unwrap().unwrap().is_empty());
    state.touch.begin_frame();
    state.touch.finish_frame(true);
    state.touch.begin_frame();
    state.touch.begin_callback();
    assert_eq!(state.read_presses().unwrap().unwrap(), ["demo-menu"]);
    assert_eq!(state.read_presses().unwrap().unwrap(), ["demo-menu"]);
    state.touch.finish_frame(false);
    assert!(state.touch.delivered);
    state.touch.begin_frame();
    assert!(state.read_presses().unwrap().unwrap().is_empty());
    state.touch.finish_frame(false);
    assert!(!state.touch.delivered);
}

/// The queue is bounded, removed buttons drop their pending taps and revocation clears all.
#[test]
fn taps_are_bounded_and_expire_with_their_buttons() {
    let mut state = state(true);
    state.touch.begin_callback();
    state
        .set_buttons(vec![button("keep"), button("drop")])
        .unwrap()
        .unwrap();
    state.touch.commit();
    for _ in 0..MAX_QUEUED_TOUCH_PRESSES - 1 {
        assert!(state.touch.queue("keep"));
    }
    assert!(state.touch.queue("drop"));
    assert!(!state.touch.queue("keep"));
    state.touch.begin_callback();
    state.set_buttons(vec![button("keep")]).unwrap().unwrap();
    state.touch.commit();
    assert!(state.touch.presses.iter().all(|id| id == "keep"));
    let owner = state.touch.owner;
    state.touch.revoke();
    assert!(state.touch.committed.is_empty() && state.touch.presses.is_empty());
    assert_eq!(state.touch.owner, owner);
    assert_ne!(super::TouchState::default().owner, owner);
}

/// A callback that removes the tapped button still counts as having received the tap.
#[test]
fn delivery_is_recorded_before_the_callback_removes_its_button() {
    let mut state = state(true);
    state.touch.begin_callback();
    state
        .set_buttons(vec![button("demo-menu")])
        .unwrap()
        .unwrap();
    state.touch.commit();
    assert!(state.touch.queue("demo-menu"));
    state.touch.begin_frame();
    state.touch.begin_callback();
    assert_eq!(state.read_presses().unwrap().unwrap(), ["demo-menu"]);
    state.set_buttons(Vec::new()).unwrap().unwrap();
    state.touch.commit();
    state.touch.finish_frame(false);
    assert!(state.touch.delivered);
}
