//! Generic action-verdict and projection regressions.

use super::*;

/// Creates a physical frame without a device, window or clock.
fn frame(attack: ActionPhase, item_use: ActionPhase) -> ActionSnapshot {
    let mut phases = [ActionPhase::default(); Action::COUNT];
    phases[Action::Attack as usize] = attack;
    phases[Action::Use as usize] = item_use;
    ActionSnapshot {
        ordered: None,
        admitted_desktop: Vec::new(),
        frame_sequence: crate::identity::InputSequence::new(1),
        authority_generation: crate::identity::InputAuthorityGeneration::INITIAL,
        movement: [0.; 2],
        raw_movement: [0.; 2],
        analogue_movement: [0.; 2],
        movement_buttons: Default::default(),
        look_delta: [0.; 2],
        input_mode: crate::InputMode::KeyboardMouse,
        phases,
        release_reasons: [None; Action::COUNT],
        authority_released: false,
    }
}

/// Projects attack/use ownership while leaving the other buttons up.
fn buttons(held: [bool; 2]) -> ActionButtons {
    project(ActionButtons::default(), held)
}

/// Adds desktop transitions with consistent physical event-time button snapshots.
fn ordered(initial: [bool; 2], events: &[(Action, bool)]) -> ActionSnapshot {
    let mut held = initial;
    let mut order = crate::OrderedActions::default();
    order.initial_buttons = buttons(held);
    let mut input = frame(Default::default(), Default::default());
    for &(action, pressed) in events {
        let phase = &mut input.phases[action as usize];
        if pressed {
            phase.pressed = true;
        } else {
            phase.released = true;
        }
        if let Some(i) = FilterAction::ALL
            .map(|a| a.action())
            .iter()
            .position(|a| *a == action)
        {
            held[i] = pressed;
        }
        order.push(ActionEvent {
            action,
            pressed,
            buttons: buttons(held),
        });
    }
    for (i, action) in FilterAction::ALL
        .map(|a| a.action())
        .into_iter()
        .enumerate()
    {
        input.phases[action as usize].held = held[i];
    }
    input.ordered = Some(order);
    input
}

/// Accepts an arbitrary transition mask without any native timing policy.
fn decide(mask: &[bool], release: [bool; 2]) -> Option<ActionVerdict> {
    Some(ActionVerdict {
        accepted: mask.to_vec(),
        release,
    })
}

#[test]
fn suppressed_release_and_press_preserve_hold_and_unrelated_snapshots() {
    let mut state = ActionFilterState::default();
    let mut input = ordered(
        [false; 2],
        &[
            (Action::Attack, true),
            (Action::Attack, false),
            (Action::Attack, true),
            (Action::Jump, true),
        ],
    );
    state.filter(&mut input, Some(1), 0, [false; 2], |_| {
        decide(&[true, false, false], [false; 2])
    });
    assert_eq!(
        input.phases[Action::Attack as usize],
        ActionPhase {
            pressed: true,
            held: true,
            released: false
        }
    );
    let events = input.ordered.unwrap().events;
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|e| e.buttons.held(Action::Attack)));
}
#[test]
fn retained_release_completes_before_a_new_press() {
    let mut state = ActionFilterState::default();
    let mut first = ordered(
        [false; 2],
        &[(Action::Attack, true), (Action::Attack, false)],
    );
    state.filter(&mut first, Some(1), 0, [false; 2], |_| {
        decide(&[true, false], [false; 2])
    });
    let mut next = ordered([false; 2], &[(Action::Jump, true), (Action::Attack, true)]);
    state.filter(&mut next, Some(1), 1, [false; 2], |f| {
        assert_eq!(f.logical, [true, false]);
        decide(&[true], [false; 2])
    });
    let events = next.ordered.unwrap().events;
    assert_eq!(
        events
            .iter()
            .map(|e| (e.action, e.pressed))
            .collect::<Vec<_>>(),
        [
            (Action::Jump, true),
            (Action::Attack, false),
            (Action::Attack, true)
        ]
    );
    assert!(events[0].buttons.held(Action::Attack));
    assert!(!events[1].buttons.held(Action::Attack));
    assert!(events[2].buttons.held(Action::Attack));
}
#[test]
fn explicit_releases_keep_both_channels_consistent() {
    let mut state = ActionFilterState::default();
    let mut first = ordered([true; 2], &[(Action::Attack, false), (Action::Use, false)]);
    state.filter(&mut first, Some(1), 0, [false; 2], |_| {
        decide(&[false, false], [false; 2])
    });
    let mut next = ordered([false; 2], &[(Action::Jump, true)]);
    state.filter(&mut next, Some(1), 1, [false; 2], |_| {
        decide(&[], [true; 2])
    });
    let events = next.ordered.unwrap().events;
    assert_eq!(events.len(), 3);
    assert!(!events[0].buttons.held(Action::Attack));
    assert!(events[0].buttons.held(Action::Use));
    assert!(!events[1].buttons.held(Action::Use));
    assert!(!events[2].buttons.held(Action::Attack));
}
#[test]
fn suppressed_press_remains_blocked_across_failure_until_release() {
    let mut state = ActionFilterState::default();
    let mut first = ordered([false; 2], &[(Action::Attack, true)]);
    state.filter(&mut first, Some(1), 0, [false; 2], |_| {
        decide(&[false], [false; 2])
    });
    state.reset();
    let mut held = ordered([true, false], &[]);
    state.filter(&mut held, None, 1, [false; 2], |_| panic!("no owner"));
    assert_eq!(held.phases[Action::Attack as usize], ActionPhase::default());
    let mut end = ordered([true, false], &[(Action::Attack, false)]);
    state.filter(&mut end, None, 2, [false; 2], |_| panic!("no owner"));
    assert!(end.ordered.unwrap().events.is_empty());
    let mut press = ordered([false; 2], &[(Action::Attack, true)]);
    state.filter(&mut press, None, 3, [false; 2], |_| panic!("no owner"));
    assert!(press.phases[Action::Attack as usize].pressed);
}
#[test]
fn failure_and_malformed_verdict_complete_retained_release_without_new_presses() {
    for malformed in [false, true] {
        let mut state = ActionFilterState::default();
        let mut first = ordered([true, false], &[(Action::Attack, false)]);
        state.filter(&mut first, Some(1), 0, [false; 2], |_| {
            decide(&[false], [false; 2])
        });
        let mut idle = ordered([false; 2], &[]);
        state.filter(&mut idle, Some(1), 1, [false; 2], |_| {
            if malformed {
                decide(&[true], [false; 2])
            } else {
                None
            }
        });
        assert_eq!(
            idle.phases[Action::Attack as usize],
            ActionPhase {
                released: true,
                ..Default::default()
            }
        );
    }
}
#[test]
fn ownership_and_authority_changes_reconcile_before_the_new_callback() {
    for reset in 0..4 {
        let mut state = ActionFilterState::default();
        let mut first = ordered([true, false], &[(Action::Attack, false)]);
        let mut epoch = 0;
        state.filter(&mut first, Some(1), 0, [false; 2], |f| {
            epoch = f.epoch;
            decide(&[false], [false; 2])
        });
        let mut next = ordered([false; 2], &[(Action::Attack, true)]);
        if reset == 1 {
            state.reset();
        }
        if reset == 2 {
            next.authority_generation = crate::identity::InputAuthorityGeneration::new(
                core::num::NonZeroU64::new(2).unwrap(),
            );
        }
        if reset == 3 {
            next.ordered.as_mut().unwrap().invalidate();
        }
        state.filter(
            &mut next,
            Some(if reset == 0 { 2 } else { 1 }),
            1,
            [false; 2],
            |f| {
                assert_ne!(f.epoch, epoch);
                assert_eq!(f.logical, [false; 2]);
                decide(&[true], [false; 2])
            },
        );
        assert!(next.phases[Action::Attack as usize].released);
        assert_eq!(next.phases[Action::Attack as usize].pressed, reset != 3);
    }
}
#[test]
fn authorized_repeat_is_independent_of_physical_edges_and_rejection_keeps_hold() {
    for accept in [true, false] {
        let mut state = ActionFilterState::default();
        let mut input = ordered([true, false], &[]);
        let admitted = state.filter(&mut input, Some(1), 0, [true, false], |f| {
            assert_eq!(
                f.transitions,
                [ActionTransition {
                    action: FilterAction::Attack,
                    kind: TransitionKind::Repeat,
                    held_after: true,
                }]
            );
            decide(&[accept], [false; 2])
        });
        assert_eq!(admitted, [accept, false]);
        assert!(input.phases[Action::Attack as usize].held);
        assert_eq!(input.phases[Action::Attack as usize].pressed, accept);
    }
    let mut state = ActionFilterState::default();
    let mut released = ordered([false; 2], &[]);
    assert_eq!(
        state.filter(&mut released, Some(1), 0, [true; 2], |_| panic!(
            "unheld repeat"
        )),
        [false; 2]
    );
}
#[test]
fn phase_only_devices_preserve_masks_and_idle_physical_holds_skip_callbacks() {
    let mut state = ActionFilterState::default();
    let mut input = frame(
        ActionPhase {
            pressed: true,
            held: true,
            ..Default::default()
        },
        Default::default(),
    );
    state.filter(&mut input, Some(1), 0, [false; 2], |_| {
        decide(&[true], [false; 2])
    });
    let mut idle = frame(
        ActionPhase {
            held: true,
            ..Default::default()
        },
        Default::default(),
    );
    state.filter(&mut idle, Some(1), 1, [false; 2], |_| panic!("idle hold"));
    assert!(idle.phases[Action::Attack as usize].held);
    let mut release = frame(
        ActionPhase {
            released: true,
            ..Default::default()
        },
        Default::default(),
    );
    state.filter(&mut release, Some(1), 2, [false; 2], |_| {
        decide(&[false], [false; 2])
    });
    assert!(release.phases[Action::Attack as usize].held);
    let mut done = frame(Default::default(), Default::default());
    state.filter(&mut done, Some(1), 3, [false; 2], |_| {
        decide(&[], [true, false])
    });
    assert!(done.phases[Action::Attack as usize].released);
}

/// Use pulses reach the filter as item-use repeats after attack's, each decided on its own.
#[test]
fn item_use_repeats_are_decided_beside_attack_repeats() {
    let mut state = ActionFilterState::default();
    let mut input = ordered([true, true], &[]);
    let admitted = state.filter(&mut input, Some(1), 0, [true; 2], |f| {
        assert_eq!(
            f.transitions,
            FilterAction::ALL.map(|action| ActionTransition {
                action,
                kind: TransitionKind::Repeat,
                held_after: true,
            })
        );
        decide(&[false, true], [false; 2])
    });
    assert_eq!(admitted, [false, true]);
    assert!(!input.phases[Action::Attack as usize].pressed);
    let item_use = input.phases[Action::Use as usize];
    assert!(item_use.pressed && item_use.held);
    let events = &input.ordered.as_ref().unwrap().events;
    assert_eq!(events.len(), 1);
    assert_eq!((events[0].action, events[0].pressed), (Action::Use, true));
    let mut unowned = ActionFilterState::default();
    let mut passthrough = ordered([false, true], &[]);
    assert_eq!(
        unowned.filter(&mut passthrough, None, 0, [true; 2], |_| panic!("no owner")),
        [false, true],
        "without a filter a held control admits its own repeat"
    );
    let mut failed = ActionFilterState::default();
    let mut dropped = ordered([true, true], &[(Action::Attack, false)]);
    assert_eq!(
        failed.filter(&mut dropped, Some(1), 0, [false, true], |_| None),
        [false; 2]
    );
    assert!(!dropped.phases[Action::Use as usize].pressed);
}

/// Failures restore physical input while dropping callback-generated pulses.
#[test]
fn failed_callback_cannot_publish_an_earlier_mod_pulse() {
    let mut state = ActionFilterState::default();
    let mut input = ordered([true, false], &[]);
    assert_eq!(
        state.filter(&mut input, Some(1), 0, [true, false], |_| None),
        [false; 2]
    );
    assert!(input.phases[Action::Attack as usize].held);
    assert!(!input.phases[Action::Attack as usize].pressed);
}

/// A suppressed gesture cannot resume from repeated press notifications alone.
#[test]
fn duplicate_physical_press_does_not_restart_a_suppressed_hold() {
    let mut state = ActionFilterState::default();
    let mut first = ordered([false; 2], &[(Action::Attack, true)]);
    state.filter(&mut first, Some(1), 0, [false; 2], |_| {
        decide(&[false], [false; 2])
    });
    let mut duplicate = ordered([true, false], &[(Action::Attack, true)]);
    state.filter(&mut duplicate, Some(1), 1, [false; 2], |_| {
        decide(&[true], [false; 2])
    });
    assert_eq!(
        duplicate.phases[Action::Attack as usize],
        ActionPhase::default()
    );
}

/// A release edge may leave the action physically held by a different binding.
#[test]
fn releasing_one_binding_preserves_another_binding_hold() {
    let mut state = ActionFilterState::default();
    let mut input = ordered([true, false], &[]);
    input.phases[Action::Attack as usize] = ActionPhase {
        held: true,
        released: true,
        pressed: true,
    };
    input.ordered.as_mut().unwrap().events = vec![
        ActionEvent {
            action: Action::Attack,
            pressed: false,
            buttons: buttons([true, false]),
        },
        ActionEvent {
            action: Action::Attack,
            pressed: true,
            buttons: buttons([true, false]),
        },
    ];
    state.filter(&mut input, Some(1), 0, [false; 2], |f| {
        assert!(f.transitions.iter().all(|t| t.held_after));
        decide(&[true, true], [false; 2])
    });
    assert!(input.phases[Action::Attack as usize].held);
    assert!(
        input
            .ordered
            .unwrap()
            .events
            .iter()
            .all(|e| e.buttons.held(Action::Attack))
    );
}

/// Momentary source edges remain taps rather than becoming an invented hold.
#[test]
fn accepted_momentary_edges_preserve_the_raw_projection() {
    let mut state = ActionFilterState::default();
    let mut input = ordered(
        [false; 2],
        &[(Action::Attack, true), (Action::Attack, false)],
    );
    for event in &mut input.ordered.as_mut().unwrap().events {
        event.buttons = buttons([false; 2]);
    }
    let original = input.phases[Action::Attack as usize];
    state.filter(&mut input, Some(1), 0, [false; 2], |f| {
        Some(ActionVerdict::passthrough(f))
    });
    assert_eq!(input.phases[Action::Attack as usize], original);
    assert_eq!(input.ordered.unwrap().events.len(), 2);
}

/// A phase-only release that leaves another binding down keeps the logical hold.
#[test]
fn phase_only_release_with_another_binding_held_keeps_the_hold() {
    let mut state = ActionFilterState::default();
    let held = ActionPhase {
        pressed: true,
        held: true,
        ..Default::default()
    };
    let mut press = frame(held, Default::default());
    state.filter(&mut press, Some(1), 0, [false; 2], |f| {
        Some(ActionVerdict::passthrough(f))
    });
    let mut release = frame(
        ActionPhase {
            released: true,
            held: true,
            ..Default::default()
        },
        Default::default(),
    );
    state.filter(&mut release, Some(1), 1, [false; 2], |f| {
        assert!(f.transitions.iter().all(|t| t.held_after));
        Some(ActionVerdict::passthrough(f))
    });
    assert!(release.phases[Action::Attack as usize].held);
    let mut idle = frame(
        ActionPhase {
            held: true,
            ..Default::default()
        },
        Default::default(),
    );
    state.filter(&mut idle, Some(1), 2, [false; 2], |_| panic!("idle hold"));
    assert!(idle.phases[Action::Attack as usize].held);
}

/// An authorized repeat cannot revive a gesture whose fresh press was suppressed.
#[test]
fn repeat_does_not_revive_a_suppressed_press() {
    for owner in [Some(1), None] {
        let mut state = ActionFilterState::default();
        let mut first = ordered([false; 2], &[(Action::Attack, true)]);
        state.filter(&mut first, Some(1), 0, [false; 2], |_| {
            decide(&[false], [false; 2])
        });
        assert!(!first.phases[Action::Attack as usize].held);
        let mut repeat = ordered([true, false], &[]);
        let admitted = state.filter(&mut repeat, owner, 1, [true, false], |f| {
            Some(ActionVerdict::passthrough(f))
        });
        assert_eq!(admitted, [false; 2]);
        assert_eq!(
            repeat.phases[Action::Attack as usize],
            ActionPhase::default()
        );
    }
}

/// A hold that resolves without an edge after a device change follows its source.
#[test]
fn device_change_hold_without_an_edge_is_not_suppressed() {
    for owner in [Some(1), None] {
        let mut state = ActionFilterState::default();
        let mut neutral = frame(Default::default(), Default::default());
        state.filter(&mut neutral, owner, 0, [false; 2], |f| {
            Some(ActionVerdict::passthrough(f))
        });
        for time in 1..3 {
            let mut mouse = ordered([true, false], &[]);
            state.filter(&mut mouse, owner, time, [false; 2], |f| {
                Some(ActionVerdict::passthrough(f))
            });
            assert!(mouse.phases[Action::Attack as usize].held);
        }
    }
}

/// An already-held controller control that resumes without an edge follows its source.
#[test]
fn edge_free_controller_hold_after_a_device_change_is_not_suppressed() {
    for owner in [Some(1), None] {
        let mut state = ActionFilterState::default();
        let mut mouse = ordered([false; 2], &[]);
        state.filter(&mut mouse, owner, 0, [false; 2], |f| {
            Some(ActionVerdict::passthrough(f))
        });
        let held = ActionPhase {
            held: true,
            ..Default::default()
        };
        for time in 1..3 {
            let mut controller = frame(held, Default::default());
            state.filter(&mut controller, owner, time, [false; 2], |f| {
                Some(ActionVerdict::passthrough(f))
            });
            assert!(controller.phases[Action::Attack as usize].held);
        }
    }
}

/// A controller hold kept while desktop input is briefly selected resumes without an edge.
#[test]
fn controller_hold_resumes_after_a_neutral_desktop_frame() {
    for owner in [Some(1), None] {
        let mut state = ActionFilterState::default();
        let pressed = ActionPhase {
            pressed: true,
            held: true,
            ..Default::default()
        };
        let mut controller = frame(pressed, Default::default());
        state.filter(&mut controller, owner, 0, [false; 2], |f| {
            Some(ActionVerdict::passthrough(f))
        });
        assert!(controller.phases[Action::Attack as usize].held);
        let mut desktop = ordered([false; 2], &[]);
        state.filter(&mut desktop, owner, 1, [false; 2], |f| {
            Some(ActionVerdict::passthrough(f))
        });
        let held = ActionPhase {
            held: true,
            ..Default::default()
        };
        for time in 2..4 {
            let mut controller = frame(held, Default::default());
            state.filter(&mut controller, owner, time, [false; 2], |f| {
                Some(ActionVerdict::passthrough(f))
            });
            assert!(controller.phases[Action::Attack as usize].held);
        }
    }
}

/// Releasing one controller binding after desktop input keeps the other binding's hold.
#[test]
fn controller_release_only_sample_after_desktop_input_keeps_its_hold() {
    for owner in [Some(1), None] {
        let mut state = ActionFilterState::default();
        let mut desktop = ordered([false; 2], &[]);
        state.filter(&mut desktop, owner, 0, [false; 2], |f| {
            Some(ActionVerdict::passthrough(f))
        });
        let released = ActionPhase {
            released: true,
            held: true,
            ..Default::default()
        };
        let mut controller = frame(released, Default::default());
        state.filter(&mut controller, owner, 1, [false; 2], |f| {
            Some(ActionVerdict::passthrough(f))
        });
        assert!(controller.phases[Action::Attack as usize].held);
        let held = ActionPhase {
            held: true,
            ..Default::default()
        };
        let mut next = frame(held, Default::default());
        state.filter(&mut next, owner, 2, [false; 2], |f| {
            Some(ActionVerdict::passthrough(f))
        });
        assert!(next.phases[Action::Attack as usize].held);
    }
}

/// A rapid press, release and press from rest keeps its coalesced release.
#[test]
fn phase_only_rapid_taps_from_rest_keep_their_release() {
    let mut state = ActionFilterState::default();
    let mut rest = frame(Default::default(), Default::default());
    state.filter(&mut rest, Some(1), 0, [false; 2], |f| {
        Some(ActionVerdict::passthrough(f))
    });
    let tapped = ActionPhase {
        pressed: true,
        released: true,
        held: true,
    };
    let mut input = frame(tapped, Default::default());
    state.filter(&mut input, Some(1), 1, [false; 2], |f| {
        assert_eq!(
            f.transitions.iter().map(|t| t.kind).collect::<Vec<_>>(),
            [
                TransitionKind::Press,
                TransitionKind::Release,
                TransitionKind::Press
            ]
        );
        Some(ActionVerdict::passthrough(f))
    });
    assert_eq!(input.phases[Action::Attack as usize], tapped);
}

/// A mouse hold kept through a neutral controller sample still follows its source on return.
#[test]
fn mouse_hold_survives_a_neutral_controller_sample() {
    for owner in [Some(1), None] {
        let mut state = ActionFilterState::default();
        let passthrough = |f: &ActionFilterFrame| Some(ActionVerdict::passthrough(f));
        let mut mouse = ordered([false; 2], &[(Action::Attack, true)]);
        state.filter(&mut mouse, owner, 0, [false; 2], passthrough);
        assert!(mouse.phases[Action::Attack as usize].held);
        let mut controller = frame(Default::default(), Default::default());
        state.filter(&mut controller, owner, 1, [false; 2], passthrough);
        let mut controller = frame(Default::default(), Default::default());
        state.filter(&mut controller, owner, 2, [false; 2], passthrough);
        let mut back = ordered([true, false], &[]);
        state.filter(&mut back, owner, 3, [false; 2], passthrough);
        assert!(back.phases[Action::Attack as usize].held);
        assert!(back.ordered.unwrap().initial_buttons.held(Action::Attack));
    }
}

/// An owner change keeps known controller holds, so a second binding cannot restart a
/// suppressed gesture.
#[test]
fn owner_change_keeps_a_suppressed_phase_only_hold_blocked() {
    for next in [Some(2), None] {
        let mut state = ActionFilterState::default();
        let press = ActionPhase {
            pressed: true,
            held: true,
            ..Default::default()
        };
        let mut first = frame(press, Default::default());
        state.filter(&mut first, Some(1), 0, [false; 2], |_| {
            decide(&[false], [false; 2])
        });
        assert!(!first.phases[Action::Attack as usize].held);
        let mut second = frame(press, Default::default());
        state.filter(&mut second, next, 1, [false; 2], |f| {
            Some(ActionVerdict::passthrough(f))
        });
        assert_eq!(
            second.phases[Action::Attack as usize],
            ActionPhase::default()
        );
    }
}

/// A controller hold that ends as desktop input is selected still reaches the filter as a
/// release, so accepting it ends the logical hold.
#[test]
fn aggregate_release_on_a_device_change_reaches_the_filter() {
    let mut state = ActionFilterState::default();
    let pressed = ActionPhase {
        pressed: true,
        held: true,
        ..Default::default()
    };
    let mut controller = frame(pressed, Default::default());
    state.filter(&mut controller, Some(1), 0, [false; 2], |f| {
        Some(ActionVerdict::passthrough(f))
    });
    let mut desktop = ordered([false; 2], &[(Action::Jump, true)]);
    desktop.phases[Action::Attack as usize].released = true;
    state.filter(&mut desktop, Some(1), 1, [false; 2], |f| {
        assert_eq!(
            f.transitions,
            [ActionTransition {
                action: FilterAction::Attack,
                kind: TransitionKind::Release,
                held_after: false,
            }]
        );
        decide(&[true], [false; 2])
    });
    assert!(!desktop.phases[Action::Attack as usize].held);
    assert!(desktop.phases[Action::Attack as usize].released);
    let events = desktop.ordered.unwrap().events;
    assert_eq!(events.len(), 2);
    assert!(events[0].action == Action::Attack && !events[0].pressed);
}

/// Tapping a second controller binding while the first stays held keeps the hold even when
/// the filter suppresses the extra press.
#[test]
fn coalesced_tap_on_a_second_binding_keeps_the_held_binding() {
    let mut state = ActionFilterState::default();
    let pressed = ActionPhase {
        pressed: true,
        held: true,
        ..Default::default()
    };
    let mut first = frame(pressed, Default::default());
    state.filter(&mut first, Some(1), 0, [false; 2], |f| {
        Some(ActionVerdict::passthrough(f))
    });
    let tapped = ActionPhase {
        pressed: true,
        released: true,
        held: true,
    };
    let mut second = frame(tapped, Default::default());
    state.filter(&mut second, Some(1), 1, [false; 2], |f| {
        assert!(f.transitions.iter().all(|t| t.held_after));
        Some(ActionVerdict {
            accepted: f
                .transitions
                .iter()
                .map(|t| t.kind != TransitionKind::Press)
                .collect(),
            release: [false; 2],
        })
    });
    assert!(second.phases[Action::Attack as usize].held);
}
