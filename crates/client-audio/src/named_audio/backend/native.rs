//! Default-device changes replace the transport while retaining every mixer source.

use super::{CAPTURE_CHANNELS, CaptureMixer, MIXER_RATE};
#[cfg(not(any(windows, target_os = "macos")))]
use rodio::cpal::traits::DeviceTrait;
use rodio::{OutputStream, Source, cpal::traits::HostTrait, dynamic_mixer};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};

/// Device checks and retries stay off the game thread.
const DEVICE_CHECK_INTERVAL: Duration = Duration::from_millis(500);
/// Consecutive failed checks, about three seconds, after which an outage is no longer a
/// default-device handoff and the player is told.
const SUSTAINED_FAILED_CHECKS: u32 = 6;
type MixerSlot = Arc<Mutex<Option<CaptureMixer>>>;
pub(super) type Opening<H = SharedOutput> = Receiver<Option<(H, Sender<()>)>>;

/// What the owner thread shares with the game thread once the output route exists.
pub(super) struct SharedOutput {
    pub(super) mixer: Arc<dynamic_mixer::DynamicMixerController<f32>>,
    /// Set while the default output has stayed unopenable for a sustained outage.
    pub(super) unavailable: Arc<AtomicBool>,
}

/// Keeps a shared mixer reachable while its hardware stream lives on another thread.
pub(super) struct Output {
    pub(super) mixer: Arc<dynamic_mixer::DynamicMixerController<f32>>,
    pub(super) unavailable: Arc<AtomicBool>,
    pub(super) _release: Sender<()>,
}

impl Output {
    /// Whether the hardware output has been unavailable for a sustained outage.
    pub(super) fn unavailable(&self) -> bool {
        self.unavailable.load(Ordering::Relaxed)
    }

    /// Adds a source to the same mixer across hardware output changes.
    pub(super) fn play_raw(
        &self,
        source: impl Source<Item = f32> + Send + 'static,
    ) -> Result<(), rodio::PlayError> {
        // The mixer remains valid when hardware changes after a caller's readiness check.
        self.mixer.add(source);
        Ok(())
    }
}

/// Owns the mixer exclusively until its stream drops, then returns it for the next stream.
struct MixerLease {
    mixer: Option<CaptureMixer>,
    returned: MixerSlot,
}

impl Iterator for MixerLease {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        // An idle mixer must stay attached so later sounds reach the same stream.
        Some(self.mixer.as_mut().and_then(Iterator::next).unwrap_or(0.0))
    }
}

impl Source for MixerLease {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        CAPTURE_CHANNELS
    }
    fn sample_rate(&self) -> u32 {
        MIXER_RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Drop for MixerLease {
    fn drop(&mut self) {
        *self.returned.lock().unwrap() = self.mixer.take();
    }
}

/// Retains active sources even if a device disappears or opening its replacement fails.
pub(super) struct Route<D, S> {
    stream: Option<(D, S)>,
    mixer: MixerSlot,
    failures: FailureStreak,
    unavailable: Arc<AtomicBool>,
}

/// Counts consecutive failed output checks since startup or the last working stream.
#[derive(Default)]
struct FailureStreak {
    checks: u32,
}

impl FailureStreak {
    /// Records one failed check and returns whether it starts the streak; only the first
    /// failure is logged at the default level.
    fn begins(&mut self) -> bool {
        self.checks = self.checks.saturating_add(1);
        self.checks == 1
    }

    /// Whether the streak has outlasted a default-device handoff.
    fn sustained(&self) -> bool {
        self.checks >= SUSTAINED_FAILED_CHECKS
    }

    /// Ends the current streak once an output plays.
    fn end(&mut self) {
        self.checks = 0;
    }
}

impl<D, S> Route<D, S> {
    /// Replaces a changed output after the old callback has returned its mixer.
    fn refresh(
        &mut self,
        device: Option<D>,
        same: impl FnOnce(&D, &D) -> bool,
        open: impl FnOnce(&D, MixerLease) -> Option<S>,
    ) {
        if let (Some((previous, _)), Some(device)) = (&self.stream, &device)
            && same(previous, device)
        {
            return;
        }
        // Drop on the owner thread before lending the mixer to another callback.
        self.stream = None;
        let Some(device) = device else {
            return;
        };
        let Some(mixer) = self.mixer.lock().unwrap().take() else {
            return;
        };
        let source = MixerLease {
            mixer: Some(mixer),
            returned: Arc::clone(&self.mixer),
        };
        if let Some(stream) = open(&device, source) {
            self.stream = Some((device, stream));
        }
    }

    /// Updates the outage state after a refresh that failed with `failure`, if any. Returns
    /// the error to log and whether it starts a failure streak. A refresh that only waited
    /// for the old callback to return the mixer neither counts nor clears a streak.
    fn settle(&mut self, failure: Option<String>) -> Option<(String, bool)> {
        if self.stream.is_some() {
            self.failures.end();
            self.unavailable.store(false, Ordering::Relaxed);
            return None;
        }
        let error = failure?;
        let begins = self.failures.begins();
        self.unavailable
            .store(self.failures.sustained(), Ordering::Relaxed);
        Some((error, begins))
    }
}

/// Builds one persistent mixer and attempts the initial default output on its owner thread.
pub(super) fn open_default_output()
-> Option<(Route<rodio::cpal::Device, OutputStream>, SharedOutput)> {
    let (controller, mixer) = dynamic_mixer::mixer::<f32>(CAPTURE_CHANNELS, MIXER_RATE);
    let mut route = Route {
        stream: None,
        mixer: Arc::new(Mutex::new(Some(mixer))),
        failures: FailureStreak::default(),
        unavailable: Arc::default(),
    };
    refresh_output(&mut route);
    let shared = SharedOutput {
        mixer: controller,
        unavailable: Arc::clone(&route.unavailable),
    };
    Some((route, shared))
}

/// Queries the OS default without enumerating or selecting an alternate output.
pub(super) fn refresh_output(route: &mut Route<rodio::cpal::Device, OutputStream>) {
    let device = rodio::cpal::default_host().default_output_device();
    let missing = device.is_none();
    let mut failure = None;
    route.refresh(
        device,
        same_device,
        |device, source| match OutputStream::try_from_device(device) {
            Ok((stream, handle)) => {
                handle
                    .play_raw(super::priority::CallbackSource::new(source))
                    .ok()?;
                Some(stream)
            }
            Err(error) => {
                failure = Some(error.to_string());
                None
            }
        },
    );
    let failure = failure.or_else(|| missing.then(|| "no default output device".to_owned()));
    match route.settle(failure) {
        Some((error, true)) => {
            bevy::log::warn!(%error, "audio output unavailable; sounds stay queued while it retries");
        }
        Some((error, false)) => {
            bevy::log::debug!(%error, "audio output still unavailable; will retry");
        }
        None => {}
    }
}

/// Compares endpoint identities on desktop hosts that expose them, including identical names.
fn same_device(left: &rodio::cpal::Device, right: &rodio::cpal::Device) -> bool {
    #[cfg(windows)]
    {
        let rodio::cpal::platform::DeviceInner::Wasapi(left) = left.as_inner();
        let rodio::cpal::platform::DeviceInner::Wasapi(right) = right.as_inner();
        left == right
    }
    #[cfg(target_os = "macos")]
    {
        let rodio::cpal::platform::DeviceInner::CoreAudio(left) = left.as_inner();
        let rodio::cpal::platform::DeviceInner::CoreAudio(right) = right.as_inner();
        left == right
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    matches!((left.name(), right.name()), (Ok(left), Ok(right)) if left == right)
}

/// Opens and refreshes a stream on one thread until the returned release sender drops.
pub(super) fn open_on_owner_thread<S: 'static, H: Send + 'static>(
    open: impl FnOnce() -> Option<(S, H)> + Send + 'static,
    mut refresh: impl FnMut(&mut S) + Send + 'static,
) -> std::io::Result<(Opening<H>, std::thread::JoinHandle<()>)> {
    let (opened, opening) = mpsc::channel();
    let owner = std::thread::Builder::new()
        .name("audio-output".to_owned())
        .spawn(move || {
            let Some((mut stream, shared)) = open() else {
                let _ = opened.send(None);
                return;
            };
            let (release, released) = mpsc::channel::<()>();
            if opened.send(Some((shared, release))).is_ok() {
                while matches!(
                    released.recv_timeout(DEVICE_CHECK_INTERVAL),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    refresh(&mut stream);
                }
            }
            drop(stream);
        })?;
    Ok((opening, owner))
}

#[cfg(test)]
mod tests;
