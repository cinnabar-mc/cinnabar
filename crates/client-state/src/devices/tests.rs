use super::*;

#[test]
fn no_external_devices_connected() {
    let tracker = DeviceTracker::default();
    assert_eq!(tracker.counts(), (0, 0));
}

#[test]
fn one_mouse_connected() {
    let mut tracker = DeviceTracker::default();
    let mouse = RawInputDeviceInfo {
        id: 1,
        is_virtual: false,
        sources: android_sources::SOURCE_MOUSE,
        keyboard_type: android_keyboard_types::KEYBOARD_TYPE_NONE,
    };
    assert!(tracker.on_device_added(mouse));
    assert_eq!(tracker.counts(), (1, 0));
}

#[test]
fn one_keyboard_connected() {
    let mut tracker = DeviceTracker::default();
    let keyboard = RawInputDeviceInfo {
        id: 2,
        is_virtual: false,
        sources: android_sources::SOURCE_KEYBOARD,
        keyboard_type: android_keyboard_types::KEYBOARD_TYPE_ALPHABETIC,
    };
    assert!(tracker.on_device_added(keyboard));
    assert_eq!(tracker.counts(), (0, 1));
}

#[test]
fn mouse_and_keyboard_connected_together() {
    let mut tracker = DeviceTracker::default();
    let mouse = RawInputDeviceInfo {
        id: 1,
        is_virtual: false,
        sources: android_sources::SOURCE_MOUSE,
        keyboard_type: android_keyboard_types::KEYBOARD_TYPE_NONE,
    };
    let keyboard = RawInputDeviceInfo {
        id: 2,
        is_virtual: false,
        sources: android_sources::SOURCE_KEYBOARD,
        keyboard_type: android_keyboard_types::KEYBOARD_TYPE_ALPHABETIC,
    };
    assert!(tracker.on_device_added(mouse));
    assert!(tracker.on_device_added(keyboard));
    assert_eq!(tracker.counts(), (1, 1));
}

#[test]
fn one_of_multiple_mice_disconnected() {
    let mut tracker = DeviceTracker::default();
    let mouse1 = RawInputDeviceInfo {
        id: 1,
        is_virtual: false,
        sources: android_sources::SOURCE_MOUSE,
        keyboard_type: android_keyboard_types::KEYBOARD_TYPE_NONE,
    };
    let mouse2 = RawInputDeviceInfo {
        id: 2,
        is_virtual: false,
        sources: android_sources::SOURCE_MOUSE,
        keyboard_type: android_keyboard_types::KEYBOARD_TYPE_NONE,
    };
    tracker.on_device_added(mouse1);
    tracker.on_device_added(mouse2);
    assert_eq!(tracker.counts(), (2, 0));

    assert!(tracker.on_device_removed(1));
    assert_eq!(tracker.counts(), (1, 0));
}

#[test]
fn keyboard_disconnected_while_mouse_remains() {
    let mut tracker = DeviceTracker::default();
    let mouse = RawInputDeviceInfo {
        id: 1,
        is_virtual: false,
        sources: android_sources::SOURCE_MOUSE,
        keyboard_type: android_keyboard_types::KEYBOARD_TYPE_NONE,
    };
    let keyboard = RawInputDeviceInfo {
        id: 2,
        is_virtual: false,
        sources: android_sources::SOURCE_KEYBOARD,
        keyboard_type: android_keyboard_types::KEYBOARD_TYPE_ALPHABETIC,
    };
    tracker.on_device_added(mouse);
    tracker.on_device_added(keyboard);
    assert_eq!(tracker.counts(), (1, 1));

    assert!(tracker.on_device_removed(2));
    assert_eq!(tracker.counts(), (1, 0));
}

#[test]
fn all_physical_devices_disconnected() {
    let mut tracker = DeviceTracker::default();
    let mouse = RawInputDeviceInfo {
        id: 1,
        is_virtual: false,
        sources: android_sources::SOURCE_MOUSE,
        keyboard_type: android_keyboard_types::KEYBOARD_TYPE_NONE,
    };
    let keyboard = RawInputDeviceInfo {
        id: 2,
        is_virtual: false,
        sources: android_sources::SOURCE_KEYBOARD,
        keyboard_type: android_keyboard_types::KEYBOARD_TYPE_ALPHABETIC,
    };
    tracker.on_device_added(mouse);
    tracker.on_device_added(keyboard);
    assert_eq!(tracker.counts(), (1, 1));

    tracker.on_device_removed(1);
    tracker.on_device_removed(2);
    assert_eq!(tracker.counts(), (0, 0));
}

#[test]
fn virtual_keyboard_excluded() {
    let mut tracker = DeviceTracker::default();
    let virtual_kb = RawInputDeviceInfo {
        id: 0,
        is_virtual: true,
        sources: android_sources::SOURCE_KEYBOARD,
        keyboard_type: android_keyboard_types::KEYBOARD_TYPE_ALPHABETIC,
    };
    assert!(!tracker.on_device_added(virtual_kb));
    assert_eq!(tracker.counts(), (0, 0));

    // Fallback virtual device with is_virtual=false but id <= 0
    let virtual_fallback = RawInputDeviceInfo {
        id: -1,
        is_virtual: false,
        sources: android_sources::SOURCE_KEYBOARD,
        keyboard_type: android_keyboard_types::KEYBOARD_TYPE_ALPHABETIC,
    };
    assert!(!tracker.on_device_added(virtual_fallback));
    assert_eq!(tracker.counts(), (0, 0));
}

#[test]
fn duplicate_connection_notifications_do_not_corrupt_counts() {
    let mut tracker = DeviceTracker::default();
    let mouse = RawInputDeviceInfo {
        id: 1,
        is_virtual: false,
        sources: android_sources::SOURCE_MOUSE,
        keyboard_type: android_keyboard_types::KEYBOARD_TYPE_NONE,
    };
    assert!(tracker.on_device_added(mouse));
    assert_eq!(tracker.counts(), (1, 0));

    // Duplicate notification with same ID
    assert!(!tracker.on_device_added(mouse));
    assert_eq!(tracker.counts(), (1, 0));
}

#[test]
fn combo_device_single_id() {
    let mut tracker = DeviceTracker::default();
    let combo = RawInputDeviceInfo {
        id: 1,
        is_virtual: false,
        sources: android_sources::SOURCE_MOUSE | android_sources::SOURCE_KEYBOARD,
        keyboard_type: android_keyboard_types::KEYBOARD_TYPE_ALPHABETIC,
    };
    assert!(tracker.on_device_added(combo));
    assert_eq!(tracker.counts(), (1, 1));

    assert!(tracker.on_device_removed(1));
    assert_eq!(tracker.counts(), (0, 0));
}

#[test]
fn non_alphabetic_hardware_keys_excluded() {
    let mut tracker = DeviceTracker::default();
    // Power and volume buttons report SOURCE_KEYBOARD with KEYBOARD_TYPE_NON_ALPHABETIC
    let volume_buttons = RawInputDeviceInfo {
        id: 3,
        is_virtual: false,
        sources: android_sources::SOURCE_KEYBOARD,
        keyboard_type: android_keyboard_types::KEYBOARD_TYPE_NON_ALPHABETIC,
    };
    assert!(!tracker.on_device_added(volume_buttons));
    assert_eq!(tracker.counts(), (0, 0));
}

#[test]
fn atomic_roundtrip() {
    set_platform_physical_devices(3, 7);
    assert_eq!(platform_physical_devices(), (3, 7));
    set_platform_physical_devices(0, 0);
    assert_eq!(platform_physical_devices(), (0, 0));
}

#[test]
fn button_state_diffing() {
    // 1. Initial Left click
    let (pressed, released) = diff_button_state(0, mouse_buttons::BUTTON_PRIMARY);
    assert_eq!(pressed, vec![mouse_buttons::BUTTON_PRIMARY]);
    assert!(released.is_empty());

    // 2. Right click added while Left held
    let (pressed, released) = diff_button_state(
        mouse_buttons::BUTTON_PRIMARY,
        mouse_buttons::BUTTON_PRIMARY | mouse_buttons::BUTTON_SECONDARY,
    );
    assert_eq!(pressed, vec![mouse_buttons::BUTTON_SECONDARY]);
    assert!(released.is_empty());

    // 3. Left released while Right remains
    let (pressed, released) = diff_button_state(
        mouse_buttons::BUTTON_PRIMARY | mouse_buttons::BUTTON_SECONDARY,
        mouse_buttons::BUTTON_SECONDARY,
    );
    assert!(pressed.is_empty());
    assert_eq!(released, vec![mouse_buttons::BUTTON_PRIMARY]);

    // 4. Simultaneous chord press (Left + Right together)
    let (pressed, released) = diff_button_state(
        0,
        mouse_buttons::BUTTON_PRIMARY | mouse_buttons::BUTTON_SECONDARY,
    );
    assert_eq!(
        pressed,
        vec![
            mouse_buttons::BUTTON_PRIMARY,
            mouse_buttons::BUTTON_SECONDARY
        ]
    );
    assert!(released.is_empty());

    // 5. Total clear on capture loss
    let (pressed, released) = diff_button_state(
        mouse_buttons::BUTTON_PRIMARY
            | mouse_buttons::BUTTON_SECONDARY
            | mouse_buttons::BUTTON_TERTIARY,
        0,
    );
    assert!(pressed.is_empty());
    assert_eq!(
        released,
        vec![
            mouse_buttons::BUTTON_PRIMARY,
            mouse_buttons::BUTTON_SECONDARY,
            mouse_buttons::BUTTON_TERTIARY,
        ]
    );
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
    // Reset delta
    let _ = take_platform_mouse_motion();
    accumulate_platform_mouse_motion(10.5, -4.2);
    accumulate_platform_mouse_motion(2.0, 1.0);
    let (dx, dy) = take_platform_mouse_motion();
    assert!((dx - 12.5).abs() < 1e-5);
    assert!((dy - -3.2).abs() < 1e-5);
    // Queue should now be empty
    assert_eq!(take_platform_mouse_motion(), (0.0, 0.0));

    // Scroll
    let _ = take_platform_mouse_scrolls();
    push_platform_mouse_scroll(1.0, 0.0);
    push_platform_mouse_scroll(-2.5, 0.5);
    let scrolls = take_platform_mouse_scrolls();
    assert_eq!(scrolls.len(), 2);
    assert_eq!(
        scrolls[0],
        PlatformMouseWheelEvent {
            vscroll: 1.0,
            hscroll: 0.0
        }
    );
    assert_eq!(
        scrolls[1],
        PlatformMouseWheelEvent {
            vscroll: -2.5,
            hscroll: 0.5
        }
    );
    assert!(take_platform_mouse_scrolls().is_empty());
}

#[test]
fn pointer_capture_state() {
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
