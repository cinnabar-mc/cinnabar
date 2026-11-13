//! Asks Android's compositor to run the display at the rate frames are paced at. Without a
//! request the system chooses, and Android 15 and later hold an app in the game category at
//! a 60 Hz default.

use render_model::FrameRate;

/// A native window's identity and the rate requested for it; no rate accepts the system's
/// choice.
type Request = (usize, Option<FrameRate>);

/// The request the compositor was last told, so it only hears about changes.
#[derive(Debug, Default)]
struct DisplayRateRequests(Option<Request>);

impl DisplayRateRequests {
    /// Forgets the last request. Android drops a request with the window it was made on.
    fn forget(&mut self) {
        self.0 = None;
    }

    /// Records `request` and returns it when the compositor has not been told it yet.
    fn changed(&mut self, request: Request) -> Option<Request> {
        if self.0 == Some(request) {
            return None;
        }
        self.0 = Some(request);
        Some(request)
    }
}

#[cfg(target_os = "android")]
pub(crate) use native::install;

#[cfg(target_os = "android")]
mod native {
    use std::{ffi::c_void, sync::OnceLock};

    use bevy::{
        ecs::system::NonSendMarker,
        prelude::{App, IntoScheduleConfigs, Last, Local, MessageReader, Res},
        window::AppLifecycle,
    };
    use client_state::RuntimeSettings;

    use super::{
        super::{
            FramePacingEnvironment, FramePacingRuntime, PresentModeRuntime, display_rate_request,
            update_frame_pacing,
        },
        DisplayRateRequests,
    };

    /// `ANATIVEWINDOW_FRAME_RATE_COMPATIBILITY_DEFAULT`: the frames have no inherent rate, so
    /// the display may also run at a multiple of the requested one.
    const COMPATIBILITY_DEFAULT: i8 = 0;

    /// `ANativeWindow_setFrameRate`, which exists from Android 11 (API 30).
    type SetFrameRate =
        unsafe extern "C" fn(window: *mut c_void, frame_rate: f32, compatibility: i8) -> i32;

    /// Adds the request after admission policy reads current settings and window state.
    pub(crate) fn install(app: &mut App) {
        app.add_systems(Last, request_display_rate.after(update_frame_pacing));
    }

    /// Looks the function up at run time, because older supported Android versions lack it.
    fn set_frame_rate_function() -> Option<SetFrameRate> {
        static FUNCTION: OnceLock<Option<SetFrameRate>> = OnceLock::new();
        *FUNCTION.get_or_init(|| {
            // SAFETY: Both names are NUL-terminated, and a missing library or symbol is null.
            let symbol = unsafe {
                let library = libc::dlopen(c"libnativewindow.so".as_ptr(), libc::RTLD_NOW);
                let scope = if library.is_null() {
                    libc::RTLD_DEFAULT
                } else {
                    library
                };
                libc::dlsym(scope, c"ANativeWindow_setFrameRate".as_ptr())
            };
            if symbol.is_null() {
                return None;
            }
            // SAFETY: The NDK declares this symbol with exactly this signature.
            Some(unsafe { std::mem::transmute::<*mut c_void, SetFrameRate>(symbol) })
        })
    }

    /// Requests the display rate for the frame-rate target when the target or the native
    /// window changes, and again after the app resumes with a new window.
    fn request_display_rate(
        runtime: Res<FramePacingRuntime>,
        presentation: Res<PresentModeRuntime>,
        settings: Res<RuntimeSettings>,
        environment: FramePacingEnvironment,
        mut lifecycle: MessageReader<AppLifecycle>,
        mut requests: Local<DisplayRateRequests>,
        _main_thread: NonSendMarker,
    ) {
        if lifecycle.read().count() > 0 {
            requests.forget();
        }
        let Some(window) = bevy::android::ANDROID_APP
            .get()
            .and_then(|app| app.native_window())
        else {
            requests.forget();
            return;
        };
        let display = environment
            .display
            .0
            .with_vrr_preference(settings.user_settings_update().1.video.vrr);
        let rate = display_rate_request(
            presentation.intent(),
            presentation.limit(),
            display,
            environment.context(runtime.ignore_window_state),
        );
        let pointer = window.ptr().as_ptr().cast::<c_void>();
        let Some((_, rate)) = requests.changed((pointer as usize, rate)) else {
            return;
        };
        let Some(set_frame_rate) = set_frame_rate_function() else {
            return;
        };
        // Zero withdraws the request and accepts the system's choice.
        let hertz = rate.map_or(0.0, |rate| rate.millihertz() as f32 / 1_000.0);
        // SAFETY: `window` holds a reference to the live native window for this call.
        let status = unsafe { set_frame_rate(pointer, hertz, COMPATIBILITY_DEFAULT) };
        if status != 0 {
            bevy::log::warn!(
                status,
                hertz,
                "Android rejected the display refresh request"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hz(rate: u32) -> Option<FrameRate> {
        FrameRate::from_hz(rate)
    }

    /// The compositor hears a request once, then only when the rate or the window changes.
    #[test]
    fn a_request_is_sent_when_the_rate_or_the_window_changes() {
        let mut requests = DisplayRateRequests::default();
        let (first, second) = (0x1000, 0x2000);
        assert_eq!(requests.changed((first, hz(120))), Some((first, hz(120))));
        assert_eq!(requests.changed((first, hz(120))), None);
        assert_eq!(requests.changed((first, hz(60))), Some((first, hz(60))));
        assert_eq!(requests.changed((first, hz(60))), None);
        assert_eq!(requests.changed((second, hz(60))), Some((second, hz(60))));
        assert_eq!(requests.changed((second, None)), Some((second, None)));
        assert_eq!(requests.changed((second, None)), None);
    }

    /// A window recreated at the same address after a resume has no request of its own.
    #[test]
    fn a_forgotten_request_is_sent_again() {
        let mut requests = DisplayRateRequests::default();
        let request = (0x1000, hz(120));
        assert_eq!(requests.changed(request), Some(request));
        requests.forget();
        assert_eq!(requests.changed(request), Some(request));
        assert_eq!(requests.changed(request), None);
    }
}
