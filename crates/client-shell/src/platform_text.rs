//! Platform keyboard requests and window obstruction geometry.

use std::collections::HashSet;

use bevy::{ecs::system::NonSendMarker, prelude::*, winit::WINIT_WINDOWS};
use ui::{DpiScale, SafeArea};

/// Explicit editor activations, including retaps after the OS dismissed its keyboard.
#[derive(Resource, Default)]
pub struct KeyboardRequests(HashSet<Entity>);

impl KeyboardRequests {
    /// Requests a platform show even when text input was already enabled.
    pub fn show(&mut self, window: Entity) {
        self.0.insert(window);
    }

    /// Whether this frame requested a keyboard for the given window.
    #[cfg(test)]
    pub(crate) fn requested(&self, window: Entity) -> bool {
        self.0.contains(&window)
    }
}

/// Delivers activation requests independently of Bevy's cached enabled flag.
pub(crate) fn show_keyboard_requests(
    mut requests: ResMut<KeyboardRequests>,
    windows: Query<&Window>,
    _main_thread: NonSendMarker,
) {
    for entity in requests.0.drain() {
        if windows
            .get(entity)
            .is_ok_and(|window| window.focused && window.ime_enabled)
        {
            WINIT_WINDOWS.with_borrow(|native| {
                if let Some(window) = native.get_window(entity) {
                    window.set_ime_allowed(true);
                }
            });
        }
    }
}

/// Cutout and keyboard obstructions in logical pixels; desktop windows resize themselves.
pub fn safe_area_insets(dpi: DpiScale) -> SafeArea {
    #[cfg(target_os = "android")]
    {
        static WARNING: std::sync::Once = std::sync::Once::new();
        insets_or_warn_once(android::safe_area_insets_pixels(), dpi, &WARNING, |error| {
            warn!(
                "CinnabarActivity.safeAreaInsetsPixels() failed; UI obstruction avoidance is unavailable: {error}"
            );
        })
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = dpi;
        SafeArea::ZERO
    }
}

/// Preserves valid geometry and reports a failed read only once before falling back to zero.
#[cfg(any(target_os = "android", test))]
fn insets_or_warn_once<E>(
    result: Result<[i32; 4], E>,
    dpi: DpiScale,
    warning: &std::sync::Once,
    report: impl FnOnce(E),
) -> SafeArea {
    match result {
        Ok(pixels) => {
            let [left, top, right, bottom] = pixels.map(|pixels| pixels.max(0) as f32);
            SafeArea::from_physical(left, top, right, bottom, dpi).unwrap_or(SafeArea::ZERO)
        }
        Err(error) => {
            warning.call_once(|| report(error));
            SafeArea::ZERO
        }
    }
}

#[cfg(target_os = "android")]
mod android {
    /// Reads geometry published by the activity's UI thread without resizing the render surface.
    pub(super) fn safe_area_insets_pixels() -> jni::errors::Result<[i32; 4]> {
        let app = bevy::android::ANDROID_APP
            .get()
            .ok_or(jni::errors::Error::NullPtr(
                "Android activity is unavailable",
            ))?;
        // Bevy keeps the VM and this activity's global reference alive for the native loop.
        let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast())? };
        let mut env = vm.attach_current_thread()?;
        let activity = unsafe { jni::objects::JObject::from_raw(app.activity_as_ptr().cast()) };
        let result = env.with_local_frame(1, |env| -> jni::errors::Result<[i32; 4]> {
            let array = jni::objects::JIntArray::from(
                env.call_method(&activity, "safeAreaInsetsPixels", "()[I", &[])?
                    .l()?,
            );
            let mut pixels = [0; 4];
            env.get_int_array_region(&array, 0, &mut pixels)?;
            Ok(pixels)
        });
        if result.is_err() && env.exception_check()? {
            env.exception_clear()?;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutout_insets_follow_rotation_and_window_dpi() {
        let warning = std::sync::Once::new();
        let dpi = DpiScale::new(2.0).unwrap();
        for (pixels, expected) in [
            ([96, 24, 0, 12], [48.0, 12.0, 0.0, 6.0]),
            ([0, 24, 96, 360], [0.0, 12.0, 48.0, 180.0]),
            ([0, 24, 96, 12], [0.0, 12.0, 48.0, 6.0]),
            ([0; 4], [0.0; 4]),
        ] {
            let safe = insets_or_warn_once(Ok::<_, ()>(pixels), dpi, &warning, |_| {
                panic!("valid window geometry")
            });
            assert_eq!(
                [safe.left(), safe.top(), safe.right(), safe.bottom()],
                expected
            );
        }
    }

    #[test]
    fn failed_obstruction_reads_warn_once_and_allow_later_geometry() {
        let warning = std::sync::Once::new();
        let dpi = DpiScale::new(1.0).unwrap();
        let mut errors = Vec::new();
        for (result, expected) in [
            (Ok([96, 0, 0, 280]), [96.0, 0.0, 0.0, 280.0]),
            (Err("missing activity method"), [0.0; 4]),
            (Err("missing activity method"), [0.0; 4]),
            (Ok([0, 0, 96, 360]), [0.0, 0.0, 96.0, 360.0]),
            (Ok([0; 4]), [0.0; 4]),
        ] {
            let safe = insets_or_warn_once(result, dpi, &warning, |error| errors.push(error));
            assert_eq!(
                [safe.left(), safe.top(), safe.right(), safe.bottom()],
                expected,
            );
        }
        assert_eq!(errors, ["missing activity method"]);
    }
}
