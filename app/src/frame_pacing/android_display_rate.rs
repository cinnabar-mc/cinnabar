//! Asks Android's compositor to run the display at the rate frames are paced at. Without a
//! request the system chooses, and Android 15 and later hold an app in the game category at
//! a 60 Hz default.

use std::time::{Duration, Instant};

use render_model::FrameRate;

/// How long a rejected request waits before it is made again.
const RETRY_INTERVAL: Duration = Duration::from_secs(1);

/// A native window's identity and the rate requested for it; no rate accepts the system's
/// choice.
type Request = (usize, Option<FrameRate>);

/// The last request made and when to repeat it if the compositor rejected it. An accepted
/// request stays on the window through display-mode changes, so it is never repeated.
#[derive(Debug, Default)]
struct DisplayRateRequests(Option<(Request, Option<Instant>)>);

impl DisplayRateRequests {
    /// Forgets the last request. Android drops a request with the window it was made on.
    fn forget(&mut self) {
        self.0 = None;
    }

    /// Whether `request` should be made at `now`: it differs from the last one, or the last
    /// one was rejected and its retry is due.
    fn due(&self, request: Request, now: Instant) -> bool {
        match self.0 {
            Some((last, retry_at)) if last == request => retry_at.is_some_and(|at| now >= at),
            _ => true,
        }
    }

    /// Records that `request` was made at `now` and whether the compositor accepted it.
    fn record(&mut self, request: Request, accepted: bool, now: Instant) {
        self.0 = Some((request, (!accepted).then(|| now + RETRY_INTERVAL)));
    }
}

#[cfg(target_os = "android")]
pub(crate) use native::install;

#[cfg(target_os = "android")]
mod native {
    use std::{
        ffi::c_void,
        sync::{Once, OnceLock},
        time::Instant,
    };

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
    /// window changes, again after the app resumes with a new window, and again at the retry
    /// interval while the compositor rejects it.
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
        let request = (pointer as usize, rate);
        let now = Instant::now();
        if !requests.due(request, now) {
            return;
        }
        let Some(set_frame_rate) = set_frame_rate_function() else {
            requests.record(request, true, now);
            return;
        };
        // Zero withdraws the request and accepts the system's choice.
        let hertz = rate.map_or(0.0, |rate| rate.millihertz() as f32 / 1_000.0);
        // SAFETY: `window` holds a reference to the live native window for this call.
        let status = unsafe { set_frame_rate(pointer, hertz, COMPATIBILITY_DEFAULT) };
        requests.record(request, status == 0, now);
        if status != 0 {
            static WARNING: Once = Once::new();
            WARNING.call_once(|| {
                bevy::log::warn!(
                    status,
                    hertz,
                    "Android rejected the display refresh request"
                );
            });
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
    fn a_request_is_made_when_the_rate_or_the_window_changes() {
        let mut requests = DisplayRateRequests::default();
        let now = Instant::now();
        let (first, second) = (0x1000, 0x2000);
        for request in [
            (first, hz(120)),
            (first, hz(60)),
            (second, hz(60)),
            (second, None),
        ] {
            assert!(requests.due(request, now), "{request:?}");
            requests.record(request, true, now);
            assert!(!requests.due(request, now), "{request:?}");
            assert!(!requests.due(request, now + 10 * RETRY_INTERVAL));
        }
    }

    /// A window recreated at the same address after a resume has no request of its own.
    #[test]
    fn a_forgotten_request_is_made_again() {
        let mut requests = DisplayRateRequests::default();
        let now = Instant::now();
        let request = (0x1000, hz(120));
        requests.record(request, true, now);
        requests.forget();
        assert!(requests.due(request, now));
    }

    /// A rejected request is repeated after the retry interval, not every frame.
    #[test]
    fn a_rejected_request_is_retried_after_the_interval_until_accepted() {
        let mut requests = DisplayRateRequests::default();
        let start = Instant::now();
        let request = (0x1000, hz(120));
        requests.record(request, false, start);
        assert!(!requests.due(request, start));
        assert!(!requests.due(request, start + RETRY_INTERVAL / 2));
        assert!(requests.due(request, start + RETRY_INTERVAL));
        assert!(
            requests.due((0x1000, hz(60)), start),
            "a new rate does not wait"
        );
        requests.record(request, true, start + RETRY_INTERVAL);
        assert!(!requests.due(request, start + 2 * RETRY_INTERVAL));
    }
}
