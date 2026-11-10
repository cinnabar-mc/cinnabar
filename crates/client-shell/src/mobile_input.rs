//! Keyboard, mouse and controller input on mobile platforms, where winit and gilrs lack
//! parts of it. Desktop builds compile the platform-independent parts for their tests.

#[cfg(target_os = "android")]
mod android;
#[cfg_attr(
    not(target_os = "android"),
    allow(dead_code, reason = "Android-only mapping, tested on every host")
)]
#[cfg(any(target_os = "android", test))]
mod android_map;
#[cfg_attr(
    not(any(target_os = "android", target_os = "ios")),
    allow(dead_code, reason = "mobile-only forwarding, tested on every host")
)]
#[cfg(any(target_os = "android", target_os = "ios", test))]
mod gamepads;
#[cfg_attr(
    not(target_os = "ios"),
    allow(dead_code, reason = "iOS-only mapping, tested on every host")
)]
#[cfg(any(target_os = "ios", test))]
mod hid_keys;
#[cfg(target_os = "ios")]
mod ios;
#[cfg_attr(
    not(target_os = "ios"),
    allow(dead_code, reason = "iOS-only forwarding, tested on every host")
)]
#[cfg(any(target_os = "ios", test))]
mod window_events;

/// Installs the platform's native input sources.
pub(crate) fn configure(app: &mut bevy::prelude::App) {
    #[cfg(target_os = "android")]
    android::configure(app);
    #[cfg(target_os = "ios")]
    ios::configure(app);
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let _ = app;
}
