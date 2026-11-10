//! Process-wide queues the platform edge (Android JNI callbacks) fills from its own threads and
//! the game loop drains each frame, plus the OS pointer-capture driver.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static PLATFORM_PHYSICAL_DEVICES: AtomicU64 = AtomicU64::new(0);
static PLATFORM_MOUSE_DELTA: Mutex<(f32, f32)> = Mutex::new((0.0, 0.0));
static PLATFORM_MOUSE_EVENTS: Mutex<Vec<PlatformMouseEvent>> = Mutex::new(Vec::new());
static PLATFORM_POINTER_CAPTURED: AtomicBool = AtomicBool::new(false);
static PLATFORM_POINTER_CAPTURE_REQUESTED: Mutex<Option<bool>> = Mutex::new(None);
static CAPTURE_DRIVER: Mutex<Option<fn(bool)>> = Mutex::new(None);

/// Mouse scroll wheel event from the platform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlatformMouseWheelEvent {
    pub vscroll: f32,
    pub hscroll: f32,
}

/// Mouse button press or release event from the platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlatformMouseButtonEvent {
    pub button_mask: u32,
    pub pressed: bool,
    /// Android `KeyEvent` meta state when the button changed.
    pub meta_state: u32,
}

/// Android `KeyEvent` meta-state bits for the modifiers a mouse binding can require.
pub mod android_meta {
    pub const META_SHIFT_ON: u32 = 0x1;
    pub const META_ALT_ON: u32 = 0x2;
    pub const META_CTRL_ON: u32 = 0x1000;
    pub const META_META_ON: u32 = 0x10000;
}

/// The modifier chord an Android meta state reports as held.
pub fn android_meta_modifiers(meta_state: u32) -> semantic_input::ModifierChord {
    let held = |bit| meta_state & bit != 0;
    semantic_input::ModifierChord {
        shift: held(android_meta::META_SHIFT_ON),
        control: held(android_meta::META_CTRL_ON),
        alt: held(android_meta::META_ALT_ON),
        super_key: held(android_meta::META_META_ON),
    }
}

/// A mouse event from the platform, queued in arrival order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlatformMouseEvent {
    Button(PlatformMouseButtonEvent),
    Wheel(PlatformMouseWheelEvent),
    /// Uncaptured pointer position in physical window pixels.
    Cursor {
        x: f32,
        y: f32,
    },
}

/// Serializes unit tests that touch these process-wide queues and capture state.
#[cfg(test)]
pub(crate) fn lock_test_globals() -> std::sync::MutexGuard<'static, ()> {
    static TEST_GLOBALS: Mutex<()> = Mutex::new(());
    TEST_GLOBALS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Android mouse button constants.
pub mod mouse_buttons {
    pub const BUTTON_PRIMARY: u32 = 1 << 0; // Left
    pub const BUTTON_SECONDARY: u32 = 1 << 1; // Right
    pub const BUTTON_TERTIARY: u32 = 1 << 2; // Middle
    pub const BUTTON_BACK: u32 = 1 << 3; // Back
    pub const BUTTON_FORWARD: u32 = 1 << 4; // Forward
}

/// Maps an Android button bitmask to a Bevy `MouseButton`.
pub fn android_button_to_mouse_button(mask: u32) -> Option<bevy::input::mouse::MouseButton> {
    match mask {
        mouse_buttons::BUTTON_PRIMARY => Some(bevy::input::mouse::MouseButton::Left),
        mouse_buttons::BUTTON_SECONDARY => Some(bevy::input::mouse::MouseButton::Right),
        mouse_buttons::BUTTON_TERTIARY => Some(bevy::input::mouse::MouseButton::Middle),
        mouse_buttons::BUTTON_BACK => Some(bevy::input::mouse::MouseButton::Back),
        mouse_buttons::BUTTON_FORWARD => Some(bevy::input::mouse::MouseButton::Forward),
        _ => None,
    }
}

/// Adds relative mouse movement from the platform.
pub fn accumulate_platform_mouse_motion(dx: f32, dy: f32) {
    if let Ok(mut delta) = PLATFORM_MOUSE_DELTA.lock() {
        delta.0 += dx;
        delta.1 += dy;
    }
}

/// Takes all accumulated relative mouse movement, resetting to (0.0, 0.0).
pub fn take_platform_mouse_motion() -> (f32, f32) {
    if let Ok(mut delta) = PLATFORM_MOUSE_DELTA.lock() {
        let current = *delta;
        *delta = (0.0, 0.0);
        current
    } else {
        (0.0, 0.0)
    }
}

/// Enqueues a mouse button press or release from the platform.
///
/// A press while capture is not held forgets an ungranted capture request, so the next frame
/// that wants capture asks again: one retry per click.
pub fn push_platform_mouse_button(button_mask: u32, pressed: bool, meta_state: u32) {
    if pressed && !is_platform_pointer_captured() {
        forget_platform_pointer_capture_request();
    }
    push_platform_mouse_event(PlatformMouseEvent::Button(PlatformMouseButtonEvent {
        button_mask,
        pressed,
        meta_state,
    }));
}

/// Enqueues a mouse scroll event from the platform.
pub fn push_platform_mouse_scroll(vscroll: f32, hscroll: f32) {
    push_platform_mouse_event(PlatformMouseEvent::Wheel(PlatformMouseWheelEvent {
        vscroll,
        hscroll,
    }));
}

fn push_platform_mouse_event(event: PlatformMouseEvent) {
    if let Ok(mut queue) = PLATFORM_MOUSE_EVENTS.lock() {
        queue.push(event);
    }
}

/// Takes all queued mouse button and wheel events in arrival order.
pub fn take_platform_mouse_events() -> Vec<PlatformMouseEvent> {
    if let Ok(mut queue) = PLATFORM_MOUSE_EVENTS.lock() {
        std::mem::take(&mut *queue)
    } else {
        Vec::new()
    }
}

/// Enqueues an uncaptured pointer position in physical window pixels.
pub fn push_platform_mouse_cursor(x: f32, y: f32) {
    push_platform_mouse_event(PlatformMouseEvent::Cursor { x, y });
}

/// Forgets an ungranted enable request, after a failed driver call or before a retry, so the next
/// enable request reaches the driver.
pub fn forget_platform_pointer_capture_request() {
    if !is_platform_pointer_captured()
        && let Ok(mut requested) = PLATFORM_POINTER_CAPTURE_REQUESTED.lock()
        && *requested == Some(true)
    {
        *requested = None;
    }
}

/// Resets the internal capture request cache (primarily for unit tests).
pub fn reset_platform_pointer_capture_state() {
    PLATFORM_POINTER_CAPTURED.store(false, Ordering::Release);
    if let Ok(mut requested) = PLATFORM_POINTER_CAPTURE_REQUESTED.lock() {
        *requested = None;
    }
}

/// Sets whether the OS pointer capture is currently active.
///
/// When the platform reports capture revocation (transition from captured `true` to `false`),
/// the tracked capture request is cleared so that a subsequent capture request can retry.
pub fn set_platform_pointer_captured(captured: bool) {
    let was_captured = PLATFORM_POINTER_CAPTURED.swap(captured, Ordering::AcqRel);
    if was_captured
        && !captured
        && let Ok(mut requested) = PLATFORM_POINTER_CAPTURE_REQUESTED.lock()
    {
        *requested = None;
    }
}

/// Reads whether the OS pointer capture is currently active.
pub fn is_platform_pointer_captured() -> bool {
    PLATFORM_POINTER_CAPTURED.load(Ordering::Acquire)
}

/// Registers the platform function that requests or releases OS pointer capture.
pub fn set_capture_driver(driver: fn(bool)) {
    if let Ok(mut slot) = CAPTURE_DRIVER.lock() {
        *slot = Some(driver);
    }
}

/// Requests or releases OS pointer capture through the registered platform driver.
///
/// Deduplicates redundant requests: only invokes the underlying driver when the
/// requested state transitions (`false` -> `true` or `true` -> `false`) or when
/// a previously failed/revoked capture request needs to be retried.
pub fn request_platform_pointer_capture(enable: bool) {
    let should_invoke = {
        if let Ok(mut requested) = PLATFORM_POINTER_CAPTURE_REQUESTED.lock() {
            let is_captured = PLATFORM_POINTER_CAPTURED.load(Ordering::Acquire);
            if enable {
                // If capture is already active and we already requested it, nothing to do.
                if is_captured && *requested == Some(true) {
                    false
                } else if *requested == Some(true) && !is_captured {
                    // Previous request was true, but capture has not succeeded or was revoked.
                    // Do not re-request every frame; retry occurs when revoked or reset.
                    false
                } else {
                    *requested = Some(true);
                    true
                }
            } else {
                // Disabling capture:
                if !is_captured && *requested == Some(false) {
                    false
                } else if !is_captured && requested.is_none() {
                    // Already not captured and no pending request to enable capture: redundant release
                    *requested = Some(false);
                    false
                } else {
                    *requested = Some(false);
                    true
                }
            }
        } else {
            true
        }
    };

    if should_invoke && let Some(driver) = CAPTURE_DRIVER.lock().ok().and_then(|slot| *slot) {
        driver(enable);
    }
}

/// Sets the platform-detected physical device counts atomically.
pub fn set_platform_physical_devices(mice: u32, keyboards: u32) {
    let packed = ((mice as u64) << 32) | (keyboards as u64);
    PLATFORM_PHYSICAL_DEVICES.store(packed, Ordering::Release);
}

/// Reads the latest platform-detected physical device counts atomically.
pub fn platform_physical_devices() -> (u32, u32) {
    let packed = PLATFORM_PHYSICAL_DEVICES.load(Ordering::Acquire);
    ((packed >> 32) as u32, (packed & 0xFFFF_FFFF) as u32)
}

#[cfg(test)]
mod tests;
