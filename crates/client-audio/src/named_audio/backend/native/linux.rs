//! ALSA's default PCM can point at hardware already owned by the desktop sound server.

use cpal::traits::{DeviceTrait, HostTrait};
use rodio::{OutputStream, OutputStreamHandle, StreamError, cpal};

/// Tries ALSA's default first, then only bridges that follow the desktop server's routing.
pub(super) fn open(
    device: &cpal::Device,
) -> Result<(OutputStream, OutputStreamHandle), StreamError> {
    open_with_fallback(device, OutputStream::try_from_device, || {
        cpal::default_host()
            .devices()
            .into_iter()
            .flatten()
            .filter_map(|device| device.name().ok().map(|name| (name, device)))
    })
}

/// Enumerates lazily after failure and never falls back to an arbitrary hardware output.
fn open_with_fallback<D, S, E, I: IntoIterator<Item = (String, D)>>(
    default: &D,
    mut open: impl FnMut(&D) -> Result<S, E>,
    alternatives: impl FnOnce() -> I,
) -> Result<S, E> {
    let error = match open(default) {
        Ok(stream) => return Ok(stream),
        Err(error) => error,
    };
    for (name, device) in alternatives() {
        if matches!(name.as_str(), "pipewire" | "pulse")
            && let Ok(stream) = open(&device)
        {
            return Ok(stream);
        }
    }
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_default_does_not_enumerate_alternatives() {
        let result = open_with_fallback(
            &0,
            |device| Ok::<_, ()>(*device),
            || -> Vec<(String, i32)> { panic!("working default must not enumerate") },
        );
        assert_eq!(result, Ok(0));
    }

    #[test]
    fn unavailable_default_uses_a_working_server_bridge_only() {
        let mut attempted = Vec::new();
        let result = open_with_fallback(
            &0,
            |device| {
                attempted.push(*device);
                if *device == 3 {
                    Ok(*device)
                } else {
                    Err(*device)
                }
            },
            || {
                [
                    ("hw:0".into(), 1),
                    ("pulse".into(), 2),
                    ("pipewire".into(), 3),
                ]
            },
        );
        assert_eq!(result, Ok(3));
        assert_eq!(attempted, [0, 2, 3]);
    }

    #[test]
    fn failed_bridges_preserve_the_default_error_for_retry() {
        let result = open_with_fallback(
            &0,
            |device| Err::<(), _>(*device),
            || [("pipewire".into(), 1), ("pulse".into(), 2)],
        );
        assert_eq!(result, Err(0));
    }
}
