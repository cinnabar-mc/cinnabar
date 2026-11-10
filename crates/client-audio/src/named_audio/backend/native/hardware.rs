//! CPAL transport with owner-observed failures instead of rodio's print-only callback.

use super::{MixerLease, StreamFailure};
use rodio::{
    StreamError,
    cpal::{
        self, FromSample, SampleFormat, SizedSample,
        traits::{DeviceTrait, StreamTrait},
    },
    dynamic_mixer,
};
use std::sync::{Arc, OnceLock};

type ConfiguredOutput = (
    cpal::Stream,
    Arc<dynamic_mixer::DynamicMixerController<f32>>,
    StreamFailure,
);

/// Opens only the selected endpoint; the retained source is attached after successful startup.
pub(super) fn open(
    device: &cpal::Device,
    source: MixerLease,
) -> Result<(cpal::Stream, StreamFailure), StreamError> {
    let config = device
        .default_output_config()
        .map_err(StreamError::DefaultStreamConfigError)?;
    let output = configure(device, &config).or_else(|original| {
        // Keep rodio 0.20's format fallback order on this same endpoint.
        let mut supported: Vec<_> = device
            .supported_output_configs()
            .map_err(StreamError::SupportedStreamConfigsError)?
            .collect();
        supported.sort_by(|left, right| right.cmp_default_heuristics(left));
        for range in supported {
            let min = range.min_sample_rate();
            let max = range.max_sample_rate();
            let cd_rate = cpal::SampleRate(44_100);
            let rates = [
                Some(max),
                (min < cd_rate && cd_rate < max).then_some(cd_rate),
                Some(min),
            ];
            for rate in rates.into_iter().flatten() {
                if let Ok(output) = configure(device, &range.with_sample_rate(rate)) {
                    return Ok(output);
                }
            }
        }
        Err(StreamError::BuildStreamError(original))
    })?;
    let (stream, controller, failure) = output;
    stream.play().map_err(StreamError::PlayStreamError)?;
    controller.add(super::super::priority::CallbackSource::new(source));
    Ok((stream, failure))
}

/// Each attempted transport owns its own failure latch, including failed format fallbacks.
fn configure(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
) -> Result<ConfiguredOutput, cpal::BuildStreamError> {
    let (controller, mixer) =
        dynamic_mixer::mixer::<f32>(config.channels(), config.sample_rate().0);
    let failure = Arc::new(OnceLock::new());
    let stream = match config.sample_format() {
        SampleFormat::F32 => build::<f32>(device, config, mixer, &failure),
        SampleFormat::F64 => build::<f64>(device, config, mixer, &failure),
        SampleFormat::I8 => build::<i8>(device, config, mixer, &failure),
        SampleFormat::I16 => build::<i16>(device, config, mixer, &failure),
        SampleFormat::I32 => build::<i32>(device, config, mixer, &failure),
        SampleFormat::I64 => build::<i64>(device, config, mixer, &failure),
        SampleFormat::U8 => build::<u8>(device, config, mixer, &failure),
        SampleFormat::U16 => build::<u16>(device, config, mixer, &failure),
        SampleFormat::U32 => build::<u32>(device, config, mixer, &failure),
        SampleFormat::U64 => build::<u64>(device, config, mixer, &failure),
        _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
    }?;
    Ok((stream, controller, failure))
}

/// The error callback stores one error; the owner logs and destroys the failed stream.
fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    mut mixer: dynamic_mixer::DynamicMixer<f32>,
    failure: &StreamFailure,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let failure = Arc::clone(failure);
    device.build_output_stream(
        &config.config(),
        move |buffer: &mut [T], _| fill(buffer, &mut mixer),
        error_callback(failure),
        None,
    )
}

/// Retains the first failure without printing or accumulating repeated backend errors.
pub(super) fn error_callback(
    failure: StreamFailure,
) -> impl FnMut(cpal::StreamError) + Send + 'static {
    move |error| {
        let _ = failure.set(error);
    }
}

/// Converts hardware samples and fills idle buffers with the format's zero-amplitude value.
fn fill<T: SizedSample + FromSample<f32>>(
    buffer: &mut [T],
    source: &mut impl Iterator<Item = f32>,
) {
    for sample in buffer {
        *sample = T::from_sample(source.next().unwrap_or(0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_buffers_preserve_samples_and_use_equilibrium_when_idle() {
        fn check<T: SizedSample + FromSample<f32> + std::fmt::Debug>() {
            let input = [-0.5, 0.0, 0.5];
            let mut buffer = [T::EQUILIBRIUM; 5];
            fill(&mut buffer, &mut input.into_iter());
            for (actual, expected) in buffer[..3].iter().zip(input) {
                assert_eq!(*actual, T::from_sample(expected));
            }
            assert_eq!(&buffer[3..], &[T::EQUILIBRIUM; 2]);
        }
        check::<f32>();
        check::<f64>();
        check::<i8>();
        check::<i16>();
        check::<i32>();
        check::<i64>();
        check::<u8>();
        check::<u16>();
        check::<u32>();
        check::<u64>();
    }
}
