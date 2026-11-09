use super::*;

#[test]
fn atomic_roundtrip() {
    let _globals = lock_test_globals();
    set_platform_physical_devices(3, 7);
    assert_eq!(platform_physical_devices(), (3, 7));
    set_platform_physical_devices(0, 0);
    assert_eq!(platform_physical_devices(), (0, 0));
}

#[test]
fn button_mask_mapping() {
    assert_eq!(
        android_button_to_mouse_button(mouse_buttons::BUTTON_PRIMARY),
        Some(bevy::input::mouse::MouseButton::Left)
    );
    assert_eq!(
        android_button_to_mouse_button(mouse_buttons::BUTTON_SECONDARY),
        Some(bevy::input::mouse::MouseButton::Right)
    );
    assert_eq!(
        android_button_to_mouse_button(mouse_buttons::BUTTON_TERTIARY),
        Some(bevy::input::mouse::MouseButton::Middle)
    );
    assert_eq!(
        android_button_to_mouse_button(mouse_buttons::BUTTON_BACK),
        Some(bevy::input::mouse::MouseButton::Back)
    );
    assert_eq!(
        android_button_to_mouse_button(mouse_buttons::BUTTON_FORWARD),
        Some(bevy::input::mouse::MouseButton::Forward)
    );
    assert_eq!(android_button_to_mouse_button(0x80), None);
}

#[test]
fn mouse_motion_and_scroll_queues() {
    let _globals = lock_test_globals();
    // Reset delta
    let _ = take_platform_mouse_motion();
    accumulate_platform_mouse_motion(10.5, -4.2);
    accumulate_platform_mouse_motion(2.0, 1.0);
    let (dx, dy) = take_platform_mouse_motion();
    assert!((dx - 12.5).abs() < 1e-5);
    assert!((dy - -3.2).abs() < 1e-5);
    // Queue should now be empty
    assert_eq!(take_platform_mouse_motion(), (0.0, 0.0));

    // Buttons and wheel steps share one queue in arrival order.
    let _ = take_platform_mouse_events();
    push_platform_mouse_scroll(1.0, 0.0);
    push_platform_mouse_button(mouse_buttons::BUTTON_PRIMARY, true, 0);
    push_platform_mouse_scroll(-2.5, 0.5);
    push_platform_mouse_cursor(40.0, 30.0);
    assert_eq!(
        take_platform_mouse_events(),
        vec![
            PlatformMouseEvent::Wheel(PlatformMouseWheelEvent {
                vscroll: 1.0,
                hscroll: 0.0
            }),
            PlatformMouseEvent::Button(PlatformMouseButtonEvent {
                button_mask: mouse_buttons::BUTTON_PRIMARY,
                pressed: true,
                meta_state: 0
            }),
            PlatformMouseEvent::Wheel(PlatformMouseWheelEvent {
                vscroll: -2.5,
                hscroll: 0.5
            }),
            PlatformMouseEvent::Cursor { x: 40.0, y: 30.0 },
        ]
    );
    assert!(take_platform_mouse_events().is_empty());
}

#[test]
fn pointer_capture_state() {
    let _globals = lock_test_globals();
    reset_platform_pointer_capture_state();
    assert!(!is_platform_pointer_captured());
    set_platform_pointer_captured(true);
    assert!(is_platform_pointer_captured());
    set_platform_pointer_captured(false);
    assert!(!is_platform_pointer_captured());
}

static TEST_CALL_COUNT: Mutex<usize> = Mutex::new(0);
static TEST_LAST_CALL: Mutex<Option<bool>> = Mutex::new(None);

fn dummy_test_driver(enable: bool) {
    if let Ok(mut count) = TEST_CALL_COUNT.lock() {
        *count += 1;
    }
    if let Ok(mut last) = TEST_LAST_CALL.lock() {
        *last = Some(enable);
    }
}

fn reset_test_driver() {
    if let Ok(mut count) = TEST_CALL_COUNT.lock() {
        *count = 0;
    }
    if let Ok(mut last) = TEST_LAST_CALL.lock() {
        *last = None;
    }
    reset_platform_pointer_capture_state();
    set_capture_driver(dummy_test_driver);
}

#[test]
fn capture_deduplication_repeated_identical_requests() {
    let _globals = lock_test_globals();
    reset_test_driver();

    // First request enable = true
    request_platform_pointer_capture(true);
    assert_eq!(*TEST_CALL_COUNT.lock().unwrap(), 1);
    assert_eq!(*TEST_LAST_CALL.lock().unwrap(), Some(true));

    // Repeated identical request before capture callback
    request_platform_pointer_capture(true);
    assert_eq!(
        *TEST_CALL_COUNT.lock().unwrap(),
        1,
        "Repeated identical request (true) must not invoke driver again"
    );

    // Platform reports capture active
    set_platform_pointer_captured(true);

    // Repeated identical request while captured
    request_platform_pointer_capture(true);
    assert_eq!(
        *TEST_CALL_COUNT.lock().unwrap(),
        1,
        "Repeated identical request while captured must not invoke driver again"
    );
}

#[test]
fn capture_switching_false_to_true_and_true_to_false() {
    let _globals = lock_test_globals();
    reset_test_driver();

    // Start disabled
    request_platform_pointer_capture(false);
    // Initial request for false when not captured and no previous request:
    // It should either call false or be a no-op if already uncaptured and requested false.
    let initial_calls = *TEST_CALL_COUNT.lock().unwrap();

    // Switching false -> true
    request_platform_pointer_capture(true);
    assert_eq!(*TEST_CALL_COUNT.lock().unwrap(), initial_calls + 1);
    assert_eq!(*TEST_LAST_CALL.lock().unwrap(), Some(true));

    // Platform acknowledges capture
    set_platform_pointer_captured(true);

    // Switching true -> false
    request_platform_pointer_capture(false);
    assert_eq!(*TEST_CALL_COUNT.lock().unwrap(), initial_calls + 2);
    assert_eq!(*TEST_LAST_CALL.lock().unwrap(), Some(false));

    // Platform acknowledges release
    set_platform_pointer_captured(false);

    // Repeated false must not call driver again
    request_platform_pointer_capture(false);
    assert_eq!(*TEST_CALL_COUNT.lock().unwrap(), initial_calls + 2);
}

#[test]
fn capture_revocation_and_subsequent_retry() {
    let _globals = lock_test_globals();
    reset_test_driver();

    // Request capture
    request_platform_pointer_capture(true);
    assert_eq!(*TEST_CALL_COUNT.lock().unwrap(), 1);

    // Capture granted
    set_platform_pointer_captured(true);
    assert!(is_platform_pointer_captured());

    // Android loses focus, user presses Home or window is revoked -> set_platform_pointer_captured(false)
    set_platform_pointer_captured(false);
    assert!(!is_platform_pointer_captured());

    // Subsequent capture request must be able to retry
    request_platform_pointer_capture(true);
    assert_eq!(
        *TEST_CALL_COUNT.lock().unwrap(),
        2,
        "After capture revocation, a new request must invoke driver to retry"
    );
    assert_eq!(*TEST_LAST_CALL.lock().unwrap(), Some(true));
}

/// An enable request Android never grants is retried after the player clicks, and after a failed
/// driver call, but not on every frame.
#[test]
fn ungranted_capture_retries_after_click_or_driver_failure() {
    let _globals = lock_test_globals();
    reset_test_driver();
    let _ = take_platform_mouse_events();

    request_platform_pointer_capture(true);
    request_platform_pointer_capture(true);
    assert_eq!(*TEST_CALL_COUNT.lock().unwrap(), 1);

    push_platform_mouse_button(mouse_buttons::BUTTON_PRIMARY, true, 0);
    request_platform_pointer_capture(true);
    request_platform_pointer_capture(true);
    assert_eq!(*TEST_CALL_COUNT.lock().unwrap(), 2);

    forget_platform_pointer_capture_request();
    request_platform_pointer_capture(true);
    assert_eq!(*TEST_CALL_COUNT.lock().unwrap(), 3);
    let _ = take_platform_mouse_events();
}

#[test]
fn android_meta_state_maps_to_modifiers() {
    let chord = android_meta_modifiers(android_meta::META_SHIFT_ON | android_meta::META_CTRL_ON);
    assert!(chord.shift && chord.control && !chord.alt && !chord.super_key);
    assert_eq!(
        android_meta_modifiers(0),
        semantic_input::ModifierChord::default()
    );
}
