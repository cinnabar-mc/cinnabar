/// One finger on the attack button, with the frame's newly pressed controls.
fn attack_touch(contact_id: u64, activity_sequence: u64, pressed: bool) -> DeviceFrame {
    DeviceFrame {
        touches: vec![semantic_input::TouchContact {
            contact_id,
            activity_sequence,
            position: [0.8, 0.5],
            delta: [0.0; 2],
            hit_id: Some(semantic_input::touch::ATTACK),
        }],
        touch_presses: pressed
            .then_some(semantic_input::touch::ATTACK)
            .into_iter()
            .collect(),
        ..DeviceFrame::default()
    }
}

fn attack_phase(router: &mut SemanticInputRouter, frame: DeviceFrame) -> semantic_input::ActionPhase {
    router.route(frame).unwrap();
    router.finalize().unwrap().phases[Action::Attack as usize]
}

/// Fast tapping never shows the router a frame with the button up: a lifted finger routes for
/// one more frame, and the next finger is down by then. Each tap is still its own press.
#[test]
fn touch_taps_without_a_released_frame_between_them_each_press() {
    let mut router = SemanticInputRouter::default();
    for tap in 0..4 {
        let contact = 10 + tap;
        let pressed = attack_phase(&mut router, attack_touch(contact, 1 + tap * 2, true));
        assert!(pressed.pressed && pressed.held, "tap {tap}");
        let held = attack_phase(&mut router, attack_touch(contact, 1 + tap * 2, false));
        assert!(!held.pressed && held.held && !held.released, "tap {tap}");
    }
}

/// A press reported while the button still reads held, as when the previous finger's lift
/// still routes, is a new press.
#[test]
fn a_press_reported_on_a_held_touch_button_presses_it_again() {
    let mut router = SemanticInputRouter::default();
    assert!(attack_phase(&mut router, attack_touch(1, 1, true)).pressed);
    let mut both = attack_touch(1, 1, true);
    both.touches.push(semantic_input::TouchContact {
        contact_id: 2,
        activity_sequence: 2,
        ..both.touches[0].clone()
    });
    let second = attack_phase(&mut router, both.clone());
    assert!(second.pressed && second.held && !second.released);
    both.touch_presses.clear();
    assert!(!attack_phase(&mut router, both).pressed);
}

/// A reported press needs a routed contact on its control, so it cannot outlive a quarantined
/// finger, and malformed press lists are rejected.
#[test]
fn touch_presses_follow_their_contacts() {
    let mut router = SemanticInputRouter::default();
    let orphan = DeviceFrame {
        touch_presses: vec![semantic_input::touch::ATTACK],
        ..DeviceFrame::default()
    };
    assert!(!attack_phase(&mut router, orphan).pressed);

    assert!(attack_phase(&mut router, attack_touch(1, 1, true)).pressed);
    router.release_all(ReleaseReason::WindowFocusLost);
    router.route(attack_touch(1, 1, false)).unwrap();
    router.finalize().unwrap();
    let quarantined = attack_phase(&mut router, attack_touch(1, 1, true));
    assert!(!quarantined.pressed && !quarantined.held);

    for (presses, error) in [
        (vec![999], FrameError::UnknownTouchControl(999)),
        (
            vec![semantic_input::touch::ATTACK; 2],
            FrameError::DuplicateTouchPress(semantic_input::touch::ATTACK),
        ),
    ] {
        let mut router = SemanticInputRouter::default();
        assert_eq!(
            router.route(DeviceFrame {
                touch_presses: presses,
                ..DeviceFrame::default()
            }),
            Err(RouterError::InvalidFrame(error))
        );
    }
}
