#[test]
fn no_candidates_with_touch_preference_selects_touch() {
    let mut router = SemanticInputRouter::default();
    assert!(router.set_preferred_input_mode(Some(InputMode::Touch)));
    assert_eq!(router.preferred_input_mode(), Some(InputMode::Touch));

    router.route(DeviceFrame::default()).unwrap();
    let snapshot = router.finalize().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::Touch);
}

#[test]
fn no_candidates_with_keyboard_mouse_preference_selects_keyboard_mouse() {
    let mut router = SemanticInputRouter::default();
    // Route a touch contact so the active mode becomes Touch
    router
        .route(DeviceFrame {
            touches: vec![semantic_input::TouchContact {
                contact_id: 1,
                activity_sequence: 1,
                position: [0.5, 0.5],
                delta: [0.0, 0.0],
                hit_id: None,
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let snapshot = router.finalize().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::Touch);

    // Set preference to KeyboardMouse and route an empty frame with no candidates
    assert!(router.set_preferred_input_mode(Some(InputMode::KeyboardMouse)));
    assert_eq!(
        router.preferred_input_mode(),
        Some(InputMode::KeyboardMouse)
    );

    router.route(DeviceFrame::default()).unwrap();
    let snapshot = router.finalize().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::KeyboardMouse);
}

#[test]
fn no_preference_preserves_existing_fallback_behaviour() {
    let mut router = SemanticInputRouter::default();
    assert_eq!(router.preferred_input_mode(), None);

    // Initial default mode is KeyboardMouse
    router.route(DeviceFrame::default()).unwrap();
    assert_eq!(
        router.finalize().unwrap().input_mode,
        InputMode::KeyboardMouse
    );

    // Switch to Touch via an active touch candidate
    router
        .route(DeviceFrame {
            touches: vec![semantic_input::TouchContact {
                contact_id: 1,
                activity_sequence: 1,
                position: [0.5, 0.5],
                delta: [0.0, 0.0],
                hit_id: None,
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(router.finalize().unwrap().input_mode, InputMode::Touch);

    // Empty frame with no preference retains previous active mode (Touch)
    router.route(DeviceFrame::default()).unwrap();
    assert_eq!(router.finalize().unwrap().input_mode, InputMode::Touch);

    // Connect controller in neutral state
    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 1,
                activity_sequence: 2,
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(router.finalize().unwrap().input_mode, InputMode::Touch);

    // Active controller input now switches mode to GamePad
    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 1,
                activity_sequence: 3,
                buttons: vec![0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(router.finalize().unwrap().input_mode, InputMode::GamePad);

    // Empty frame with no preference retains previous active mode (GamePad)
    router.route(DeviceFrame::default()).unwrap();
    assert_eq!(router.finalize().unwrap().input_mode, InputMode::GamePad);
}

#[test]
fn active_candidates_take_priority_over_manual_preference() {
    let mut router = SemanticInputRouter::default();

    // Preference is Touch, but keyboard/mouse activity is present
    assert!(router.set_preferred_input_mode(Some(InputMode::Touch)));
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                mouse_motion: [5.0, 0.0],
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        })
        .unwrap();
    let snapshot = router.finalize().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::KeyboardMouse);

    // Preference is KeyboardMouse, but touch activity is present
    assert!(router.set_preferred_input_mode(Some(InputMode::KeyboardMouse)));
    router
        .route(DeviceFrame {
            touches: vec![semantic_input::TouchContact {
                contact_id: 1,
                activity_sequence: 2,
                position: [0.5, 0.5],
                delta: [0.1, 0.0],
                hit_id: None,
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let snapshot = router.finalize().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::Touch);

    // Preference is Touch, but gamepad activity is present
    assert!(router.set_preferred_input_mode(Some(InputMode::Touch)));
    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 2,
                activity_sequence: 3,
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    router.finalize().unwrap();
    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 2,
                activity_sequence: 4,
                buttons: vec![0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let snapshot = router.finalize().unwrap();
    assert_eq!(snapshot.input_mode, InputMode::GamePad);
}

#[test]
fn gamepad_cannot_be_selected_as_manual_preference() {
    let mut router = SemanticInputRouter::default();
    assert_eq!(router.preferred_input_mode(), None);

    // Attempting to set GamePad as preference is rejected
    assert!(!router.set_preferred_input_mode(Some(InputMode::GamePad)));
    assert_eq!(router.preferred_input_mode(), None);

    // Empty frame routing does not switch to GamePad
    router.route(DeviceFrame::default()).unwrap();
    let snapshot = router.finalize().unwrap();
    assert_ne!(snapshot.input_mode, InputMode::GamePad);

    // Existing preference is retained if a GamePad selection is attempted
    assert!(router.set_preferred_input_mode(Some(InputMode::Touch)));
    assert!(!router.set_preferred_input_mode(Some(InputMode::GamePad)));
    assert_eq!(router.preferred_input_mode(), Some(InputMode::Touch));
}
