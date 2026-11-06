//! Physical input device observation and tracking.

use bevy::prelude::Resource;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Physical input devices observed by the platform edge (e.g. Android JNI).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Resource)]
pub struct ConnectedPhysicalDevices {
    pub mice: u32,
    pub keyboards: u32,
}

static PLATFORM_PHYSICAL_DEVICES: AtomicU64 = AtomicU64::new(0);
static PLATFORM_MOUSE_DELTA: Mutex<(f32, f32)> = Mutex::new((0.0, 0.0));
static PLATFORM_MOUSE_BUTTONS: Mutex<Vec<PlatformMouseButtonEvent>> = Mutex::new(Vec::new());
static PLATFORM_MOUSE_SCROLLS: Mutex<Vec<PlatformMouseWheelEvent>> = Mutex::new(Vec::new());
static PLATFORM_POINTER_CAPTURED: AtomicBool = AtomicBool::new(false);
static CAPTURE_DRIVER: OnceLock<fn(bool)> = OnceLock::new();

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

/// Derives which mouse buttons were pressed and released between two button states.
pub fn diff_button_state(previous: u32, current: u32) -> (Vec<u32>, Vec<u32>) {
    let changed = previous ^ current;
    let mut pressed = Vec::new();
    let mut released = Vec::new();
    for i in 0..5 {
        let mask = 1 << i;
        if (changed & mask) != 0 {
            if (current & mask) != 0 {
                pressed.push(mask);
            } else {
                released.push(mask);
            }
        }
    }
    (pressed, released)
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
pub fn push_platform_mouse_button(button_mask: u32, pressed: bool) {
    if let Ok(mut queue) = PLATFORM_MOUSE_BUTTONS.lock() {
        queue.push(PlatformMouseButtonEvent {
            button_mask,
            pressed,
        });
    }
}

/// Takes all queued mouse button events.
pub fn take_platform_mouse_buttons() -> Vec<PlatformMouseButtonEvent> {
    if let Ok(mut queue) = PLATFORM_MOUSE_BUTTONS.lock() {
        std::mem::take(&mut *queue)
    } else {
        Vec::new()
    }
}

/// Enqueues a mouse scroll event from the platform.
pub fn push_platform_mouse_scroll(vscroll: f32, hscroll: f32) {
    if let Ok(mut queue) = PLATFORM_MOUSE_SCROLLS.lock() {
        queue.push(PlatformMouseWheelEvent { vscroll, hscroll });
    }
}

/// Takes all queued mouse scroll events.
pub fn take_platform_mouse_scrolls() -> Vec<PlatformMouseWheelEvent> {
    if let Ok(mut queue) = PLATFORM_MOUSE_SCROLLS.lock() {
        std::mem::take(&mut *queue)
    } else {
        Vec::new()
    }
}

/// Sets whether the OS pointer capture is currently active.
pub fn set_platform_pointer_captured(captured: bool) {
    PLATFORM_POINTER_CAPTURED.store(captured, Ordering::Release);
}

/// Reads whether the OS pointer capture is currently active.
pub fn is_platform_pointer_captured() -> bool {
    PLATFORM_POINTER_CAPTURED.load(Ordering::Acquire)
}

/// Registers the platform function that requests or releases OS pointer capture.
pub fn set_capture_driver(driver: fn(bool)) {
    let _ = CAPTURE_DRIVER.set(driver);
}

/// Requests or releases OS pointer capture through the registered platform driver.
pub fn request_platform_pointer_capture(enable: bool) {
    if let Some(driver) = CAPTURE_DRIVER.get() {
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

/// Android `InputDevice` source constants.
pub mod android_sources {
    pub const SOURCE_KEYBOARD: u32 = 0x0000_0101;
    pub const SOURCE_TOUCHSCREEN: u32 = 0x0000_1002;
    pub const SOURCE_MOUSE: u32 = 0x0000_2002;
    pub const SOURCE_MOUSE_RELATIVE: u32 = 0x0002_0004;
    pub const SOURCE_GAMEPAD: u32 = 0x0000_0401;
    pub const SOURCE_JOYSTICK: u32 = 0x0100_0010;
}

/// Android `InputDevice` keyboard type constants.
pub mod android_keyboard_types {
    pub const KEYBOARD_TYPE_NONE: i32 = 0;
    pub const KEYBOARD_TYPE_NON_ALPHABETIC: i32 = 1;
    pub const KEYBOARD_TYPE_ALPHABETIC: i32 = 2;
}

/// Normalized description of an input device reported by the platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawInputDeviceInfo {
    pub id: i32,
    pub is_virtual: bool,
    pub sources: u32,
    pub keyboard_type: i32,
}

impl RawInputDeviceInfo {
    /// Determines whether the device qualifies as a physical mouse/pointer.
    pub fn is_physical_mouse(&self) -> bool {
        if self.is_virtual || self.id <= 0 {
            return false;
        }
        (self.sources & android_sources::SOURCE_MOUSE) == android_sources::SOURCE_MOUSE
            || (self.sources & android_sources::SOURCE_MOUSE_RELATIVE) != 0
    }

    /// Determines whether the device qualifies as a real physical alphanumeric keyboard.
    pub fn is_physical_keyboard(&self) -> bool {
        if self.is_virtual || self.id <= 0 {
            return false;
        }
        let has_keyboard =
            (self.sources & android_sources::SOURCE_KEYBOARD) == android_sources::SOURCE_KEYBOARD;
        let is_alphabetic = self.keyboard_type == android_keyboard_types::KEYBOARD_TYPE_ALPHABETIC;
        has_keyboard && is_alphabetic
    }
}

/// Tracks connected physical devices without double-counting.
#[derive(Default, Debug)]
pub struct DeviceTracker {
    mice: HashSet<i32>,
    keyboards: HashSet<i32>,
}

impl DeviceTracker {
    /// Handles device addition. Returns true if the set of physical devices changed.
    pub fn on_device_added(&mut self, device: RawInputDeviceInfo) -> bool {
        let mut changed = false;
        if device.is_physical_mouse() {
            changed |= self.mice.insert(device.id);
        }
        if device.is_physical_keyboard() {
            changed |= self.keyboards.insert(device.id);
        }
        changed
    }

    /// Handles device removal by ID. Returns true if a tracked physical device was removed.
    pub fn on_device_removed(&mut self, device_id: i32) -> bool {
        let mouse_removed = self.mice.remove(&device_id);
        let keyboard_removed = self.keyboards.remove(&device_id);
        mouse_removed || keyboard_removed
    }

    /// Handles device change by re-evaluating classification. Returns true if counts changed.
    pub fn on_device_changed(&mut self, device: RawInputDeviceInfo) -> bool {
        self.on_device_removed(device.id);
        self.on_device_added(device)
    }

    /// Returns current counts of connected physical mice and keyboards `(mice, keyboards)`.
    pub fn counts(&self) -> (u32, u32) {
        (self.mice.len() as u32, self.keyboards.len() as u32)
    }
}

#[cfg(test)]
mod tests;
