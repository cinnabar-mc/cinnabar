//! Display facts a platform host supplies when the window system cannot report them.

use std::sync::OnceLock;

static REFRESH_RATE_SOURCE: OnceLock<fn() -> Option<f32>> = OnceLock::new();

/// Registers the host's reader of the highest refresh rate, in hertz, that the display offers
/// at its current resolution. Android's window system reports no refresh rate.
pub fn set_refresh_rate_source(source: fn() -> Option<f32>) {
    let _ = REFRESH_RATE_SOURCE.set(source);
}

/// The registered host's reading in hertz; `None` without a host source or a reading.
pub(crate) fn host_refresh_rate() -> Option<f32> {
    REFRESH_RATE_SOURCE.get().and_then(|source| source())
}
