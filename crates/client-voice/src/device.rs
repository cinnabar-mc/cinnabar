//! Microphone and speakers through cpal (WASAPI on Windows, Oboe/AAudio on Android).
//!
//! cpal streams are not `Send`, so a dedicated audio thread creates and owns them; this
//! handle only shares sample queues with it and is `Send`.
//!
//! This file needs the `cpal` crate and could not be compiled where it was written.

use crate::codec::SAMPLE_RATE;
use crate::engine::AudioIo;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Queued audio beyond this is dropped so a stalled consumer cannot add seconds of delay.
const MAX_QUEUED_FRAMES: usize = SAMPLE_RATE as usize; // one second

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What the machine offered when the devices were opened.
#[derive(Debug, Clone, Default)]
pub struct DeviceReport {
    pub microphone: Option<String>,
    pub speaker: Option<String>,
    /// Why a missing device is missing (permission denied, none present, format unsupported).
    pub problems: Vec<String>,
}

pub struct CpalIo {
    mic: Arc<Mutex<Vec<i16>>>,
    playback: Arc<Mutex<VecDeque<i16>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    report: DeviceReport,
}

impl CpalIo {
    /// Opens the default microphone and speaker. Succeeds with whichever of the two works,
    /// and fails only when neither does; see [`CpalIo::report`] for details.
    pub fn open() -> Result<Self, String> {
        let mic = Arc::new(Mutex::new(Vec::new()));
        let playback = Arc::new(Mutex::new(VecDeque::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel::<DeviceReport>();

        let (m, p, s) = (mic.clone(), playback.clone(), stop.clone());
        let worker = thread::Builder::new()
            .name("cinnabar-voice-audio".into())
            .spawn(move || {
                let mut report = DeviceReport::default();
                let input = open_input(&m, &mut report);
                let output = open_output(&p, &mut report);
                let _ = ready_tx.send(report);
                // Streams live exactly as long as this thread keeps them.
                while !s.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(50));
                }
                drop((input, output));
            })
            .map_err(|e| format!("no se pudo iniciar el hilo de audio: {e}"))?;

        let report = ready_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| "los dispositivos de audio no respondieron".to_owned())?;
        if report.microphone.is_none() && report.speaker.is_none() {
            stop.store(true, Ordering::Relaxed);
            let _ = worker.join();
            return Err(report.problems.join("; "));
        }
        Ok(Self {
            mic,
            playback,
            stop,
            worker: Some(worker),
            report,
        })
    }

    pub fn report(&self) -> &DeviceReport {
        &self.report
    }
}

impl Drop for CpalIo {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl AudioIo for CpalIo {
    fn read_mic(&mut self, out: &mut Vec<i16>) {
        out.append(&mut lock(&self.mic));
    }

    fn write_playback(&mut self, stereo: &[i16]) {
        let mut queue = lock(&self.playback);
        queue.extend(stereo.iter().copied());
        let limit = MAX_QUEUED_FRAMES * 2;
        if queue.len() > limit {
            let excess = queue.len() - limit;
            queue.drain(..excess);
        }
    }
}

// ---------------------------------------------------------------- capture

/// Converts a device's channels and sample rate into 48 kHz mono `i16`.
struct MonoResampler {
    /// Input samples consumed per output sample.
    step: f64,
    position: f64,
    previous: f32,
}

impl MonoResampler {
    fn new(input_rate: u32) -> Self {
        Self {
            step: f64::from(input_rate) / f64::from(SAMPLE_RATE),
            position: 0.0,
            previous: 0.0,
        }
    }

    fn push(&mut self, sample: f32, out: &mut Vec<i16>) {
        while self.position < 1.0 {
            let t = self.position as f32;
            let value = self.previous + (sample - self.previous) * t;
            out.push((value.clamp(-1.0, 1.0) * 32767.0) as i16);
            self.position += self.step;
        }
        self.position -= 1.0;
        self.previous = sample;
    }
}

fn open_input(sink: &Arc<Mutex<Vec<i16>>>, report: &mut DeviceReport) -> Option<Stream> {
    let host = cpal::default_host();
    let Some(device) = host.default_input_device() else {
        report.problems.push("no hay micrófono".into());
        return None;
    };
    let config = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            report
                .problems
                .push(format!("micrófono sin configuración válida: {e}"));
            return None;
        }
    };
    let format = config.sample_format();
    let stream_config: StreamConfig = config.into();
    let result = match format {
        SampleFormat::F32 => input_stream::<f32>(&device, &stream_config, sink),
        SampleFormat::I16 => input_stream::<i16>(&device, &stream_config, sink),
        SampleFormat::U16 => input_stream::<u16>(&device, &stream_config, sink),
        other => {
            report
                .problems
                .push(format!("formato de micrófono no soportado: {other:?}"));
            return None;
        }
    };
    match result.and_then(|stream| stream.play().map(|()| stream).map_err(Into::into)) {
        Ok(stream) => {
            report.microphone = Some(device.name().unwrap_or_else(|_| "micrófono".into()));
            Some(stream)
        }
        Err(e) => {
            report
                .problems
                .push(format!("no se pudo abrir el micrófono (¿permiso?): {e}"));
            None
        }
    }
}

fn input_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    sink: &Arc<Mutex<Vec<i16>>>,
) -> Result<Stream, Box<dyn std::error::Error>>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels.max(1));
    let mut resampler = MonoResampler::new(config.sample_rate.0);
    let sink = sink.clone();
    let mut converted = Vec::new();
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _| {
            converted.clear();
            for frame in data.chunks(channels) {
                let sum: f32 = frame.iter().map(|&s| f32::from_sample(s)).sum();
                resampler.push(sum / frame.len() as f32, &mut converted);
            }
            let mut queue = lock(&sink);
            queue.extend_from_slice(&converted);
            if queue.len() > MAX_QUEUED_FRAMES {
                let excess = queue.len() - MAX_QUEUED_FRAMES;
                queue.drain(..excess);
            }
        },
        |_error| {},
        None,
    )?;
    Ok(stream)
}

// ---------------------------------------------------------------- playback

fn open_output(source: &Arc<Mutex<VecDeque<i16>>>, report: &mut DeviceReport) -> Option<Stream> {
    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        report.problems.push("no hay altavoces".into());
        return None;
    };
    let config = match device.default_output_config() {
        Ok(c) => c,
        Err(e) => {
            report
                .problems
                .push(format!("altavoces sin configuración válida: {e}"));
            return None;
        }
    };
    let format = config.sample_format();
    let stream_config: StreamConfig = config.into();
    let result = match format {
        SampleFormat::F32 => output_stream::<f32>(&device, &stream_config, source),
        SampleFormat::I16 => output_stream::<i16>(&device, &stream_config, source),
        SampleFormat::U16 => output_stream::<u16>(&device, &stream_config, source),
        other => {
            report
                .problems
                .push(format!("formato de altavoz no soportado: {other:?}"));
            return None;
        }
    };
    match result.and_then(|stream| stream.play().map(|()| stream).map_err(Into::into)) {
        Ok(stream) => {
            report.speaker = Some(device.name().unwrap_or_else(|_| "altavoces".into()));
            Some(stream)
        }
        Err(e) => {
            report
                .problems
                .push(format!("no se pudo abrir los altavoces: {e}"));
            None
        }
    }
}

fn output_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    source: &Arc<Mutex<VecDeque<i16>>>,
) -> Result<Stream, Box<dyn std::error::Error>>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels.max(1));
    // Source frames consumed per output frame.
    let ratio = f64::from(SAMPLE_RATE) / f64::from(config.sample_rate.0);
    let source = source.clone();
    let (mut previous, mut next) = ((0f32, 0f32), (0f32, 0f32));
    let mut position = 1.0f64;
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            let mut queue = lock(&source);
            let mut pop = |queue: &mut VecDeque<i16>| -> (f32, f32) {
                match (queue.pop_front(), queue.pop_front()) {
                    (Some(l), Some(r)) => (f32::from(l) / 32768.0, f32::from(r) / 32768.0),
                    _ => (0.0, 0.0),
                }
            };
            for frame in data.chunks_mut(channels) {
                while position >= 1.0 {
                    previous = next;
                    next = pop(&mut queue);
                    position -= 1.0;
                }
                let t = position as f32;
                let left = previous.0 + (next.0 - previous.0) * t;
                let right = previous.1 + (next.1 - previous.1) * t;
                position += ratio;
                for (i, slot) in frame.iter_mut().enumerate() {
                    let value = match i {
                        0 => left,
                        1 => right,
                        _ => 0.0,
                    };
                    *slot = T::from_sample(value);
                }
                // Mono devices get both channels averaged instead of only the left one.
                if channels == 1 {
                    frame[0] = T::from_sample((left + right) * 0.5);
                }
            }
        },
        |_error| {},
        None,
    )?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_produces_the_expected_number_of_samples() {
        for rate in [16_000u32, 44_100, 48_000] {
            let mut r = MonoResampler::new(rate);
            let mut out = Vec::new();
            for i in 0..rate {
                r.push(((i % 100) as f32 - 50.0) / 100.0, &mut out);
            }
            let diff = (out.len() as i64 - 48_000).abs();
            assert!(diff <= 2, "{rate} Hz -> {} samples", out.len());
        }
    }
}
