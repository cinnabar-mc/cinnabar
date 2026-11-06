//! Voice codec with no native dependencies, so it builds the same on Windows and Android.
//!
//! A 20 ms, 48 kHz mono frame (960 samples) is reduced to 16 kHz (wideband speech) and coded
//! with IMA ADPCM at 4 bits per sample: 163 bytes per frame, about 65 kbit/s. Every frame
//! carries its own predictor and step index, so a lost datagram never corrupts the next one.
//!
//! Opus would use about a third of the bandwidth. It needs a C library on both targets, so it
//! is left as a later swap behind [`FrameCodec`].

use crate::codec::{FRAME_SAMPLES, FrameCodec};

const DECIMATION: usize = 3;
const CODED_SAMPLES: usize = FRAME_SAMPLES / DECIMATION;
/// predictor (2) + step index (1) + two samples per byte.
pub const ENCODED_LEN: usize = 3 + CODED_SAMPLES / 2;

const STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
const INDEX_TABLE: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

#[derive(Debug, PartialEq, Eq)]
pub enum AdpcmError {
    WrongFrameSize,
    WrongPayloadSize,
}

/// Predictor and step index: the whole ADPCM state.
#[derive(Clone, Copy)]
struct State {
    predictor: i32,
    index: i32,
}

impl State {
    /// Applies a 4-bit code, returning the reconstructed sample.
    fn apply(&mut self, code: u8) -> i16 {
        let step = STEP_TABLE[self.index as usize];
        let mut delta = step >> 3;
        if code & 4 != 0 {
            delta += step;
        }
        if code & 2 != 0 {
            delta += step >> 1;
        }
        if code & 1 != 0 {
            delta += step >> 2;
        }
        if code & 8 != 0 {
            self.predictor -= delta;
        } else {
            self.predictor += delta;
        }
        self.predictor = self.predictor.clamp(i16::MIN.into(), i16::MAX.into());
        self.index = (self.index + INDEX_TABLE[usize::from(code & 7)]).clamp(0, 88);
        self.predictor as i16
    }

    fn encode(&mut self, sample: i16) -> u8 {
        let step = STEP_TABLE[self.index as usize];
        let mut diff = i32::from(sample) - self.predictor;
        let mut code = 0u8;
        if diff < 0 {
            code = 8;
            diff = -diff;
        }
        let mut threshold = step;
        if diff >= threshold {
            code |= 4;
            diff -= threshold;
        }
        threshold >>= 1;
        if diff >= threshold {
            code |= 2;
            diff -= threshold;
        }
        threshold >>= 1;
        if diff >= threshold {
            code |= 1;
        }
        self.apply(code);
        code
    }
}

#[derive(Debug, Default)]
pub struct AdpcmCodec {
    /// Step index carried between frames so the encoder adapts smoothly.
    index: i32,
    /// Last decoded 16 kHz frame, replayed (faded) to conceal a lost one.
    last: Vec<i16>,
    conceal_run: u32,
}

impl FrameCodec for AdpcmCodec {
    type Error = AdpcmError;

    fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>, AdpcmError> {
        if pcm.len() != FRAME_SAMPLES {
            return Err(AdpcmError::WrongFrameSize);
        }
        let narrow = decimate(pcm);
        let mut state = State {
            predictor: i32::from(narrow[0]),
            index: self.index,
        };
        let mut out = Vec::with_capacity(ENCODED_LEN);
        out.extend_from_slice(&(narrow[0]).to_le_bytes());
        out.push(state.index as u8);
        let (pairs, _) = narrow.as_chunks::<2>();
        for pair in pairs {
            let low = state.encode(pair[0]);
            let high = state.encode(pair[1]);
            out.push(low | (high << 4));
        }
        self.index = state.index;
        Ok(out)
    }

    fn decode(&mut self, data: Option<&[u8]>) -> Result<Vec<i16>, AdpcmError> {
        let narrow: Vec<i16> = match data {
            Some(bytes) => {
                if bytes.len() != ENCODED_LEN || bytes[2] > 88 {
                    return Err(AdpcmError::WrongPayloadSize);
                }
                let mut state = State {
                    predictor: i32::from(i16::from_le_bytes([bytes[0], bytes[1]])),
                    index: i32::from(bytes[2]),
                };
                let mut out = Vec::with_capacity(CODED_SAMPLES);
                for byte in &bytes[3..] {
                    out.push(state.apply(byte & 0x0F));
                    out.push(state.apply(byte >> 4));
                }
                self.last.clone_from(&out);
                self.conceal_run = 0;
                out
            }
            None => {
                self.conceal_run += 1;
                // Fade each consecutive lost frame so a long gap decays to silence.
                let gain = 0.5f32.powi(self.conceal_run as i32);
                if self.last.is_empty() {
                    vec![0; CODED_SAMPLES]
                } else {
                    self.last
                        .iter()
                        .map(|&s| (f32::from(s) * gain) as i16)
                        .collect()
                }
            }
        };
        Ok(interpolate(&narrow))
    }
}

/// 48 kHz to 16 kHz by averaging each group of three samples (a crude but cheap low-pass).
fn decimate(pcm: &[i16]) -> Vec<i16> {
    pcm.as_chunks::<DECIMATION>()
        .0
        .iter()
        .map(|group| (group.iter().map(|&s| i32::from(s)).sum::<i32>() / DECIMATION as i32) as i16)
        .collect()
}

/// 16 kHz to 48 kHz with linear interpolation.
fn interpolate(narrow: &[i16]) -> Vec<i16> {
    let mut out = Vec::with_capacity(FRAME_SAMPLES);
    for (i, &current) in narrow.iter().enumerate() {
        let next = narrow.get(i + 1).copied().unwrap_or(current);
        let (a, b) = (i32::from(current), i32::from(next));
        for k in 0..DECIMATION as i32 {
            out.push((a + (b - a) * k / DECIMATION as i32) as i16);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, amplitude: f32, offset: usize) -> Vec<i16> {
        (0..FRAME_SAMPLES)
            .map(|i| {
                let t = (i + offset) as f32 / 48_000.0;
                (amplitude * (2.0 * std::f32::consts::PI * freq * t).sin()) as i16
            })
            .collect()
    }

    fn snr_db(original: &[i16], decoded: &[i16]) -> f32 {
        let signal: f64 = original.iter().map(|&s| f64::from(s).powi(2)).sum();
        let noise: f64 = original
            .iter()
            .zip(decoded)
            .map(|(&a, &b)| (f64::from(a) - f64::from(b)).powi(2))
            .sum();
        (10.0 * (signal / noise.max(1.0)).log10()) as f32
    }

    #[test]
    fn frames_have_the_documented_size() {
        let mut c = AdpcmCodec::default();
        let encoded = c.encode(&sine(440.0, 8000.0, 0)).unwrap();
        assert_eq!(encoded.len(), ENCODED_LEN);
        assert_eq!(ENCODED_LEN, 163);
        assert_eq!(c.decode(Some(&encoded)).unwrap().len(), FRAME_SAMPLES);
    }

    #[test]
    fn speech_band_tones_survive_with_good_quality() {
        // Quality falls with frequency (linear resampling plus 4-bit steps); speech energy is
        // mostly below 1 kHz, so the floor is lower for the 2.5 kHz tone.
        for (freq, floor) in [(200.0, 15.0), (440.0, 15.0), (1000.0, 15.0), (2500.0, 8.0)] {
            let mut enc = AdpcmCodec::default();
            let mut dec = AdpcmCodec::default();
            let mut worst = f32::MAX;
            for frame in 0..10 {
                let pcm = sine(freq, 9000.0, frame * FRAME_SAMPLES);
                let decoded = dec.decode(Some(&enc.encode(&pcm).unwrap())).unwrap();
                if frame >= 2 {
                    worst = worst.min(snr_db(&pcm, &decoded));
                }
            }
            assert!(worst > floor, "{freq} Hz: SNR {worst:.1} dB");
        }
    }

    #[test]
    fn silence_stays_silent() {
        let mut c = AdpcmCodec::default();
        let encoded = c.encode(&[0; FRAME_SAMPLES]).unwrap();
        let decoded = c.decode(Some(&encoded)).unwrap();
        assert!(decoded.iter().all(|&s| s.abs() <= 8));
    }

    #[test]
    fn a_lost_frame_does_not_corrupt_the_next_one() {
        let mut enc = AdpcmCodec::default();
        let mut dec = AdpcmCodec::default();
        let frames: Vec<_> = (0..6)
            .map(|f| enc.encode(&sine(440.0, 9000.0, f * FRAME_SAMPLES)).unwrap())
            .collect();
        let pcm_after = sine(440.0, 9000.0, 4 * FRAME_SAMPLES);
        dec.decode(Some(&frames[0])).unwrap();
        dec.decode(Some(&frames[1])).unwrap();
        dec.decode(None).unwrap(); // frames[2] lost
        dec.decode(None).unwrap(); // frames[3] lost
        let decoded = dec.decode(Some(&frames[4])).unwrap();
        assert!(snr_db(&pcm_after, &decoded) > 15.0);
    }

    #[test]
    fn concealment_fades_to_silence() {
        let mut enc = AdpcmCodec::default();
        let mut dec = AdpcmCodec::default();
        dec.decode(Some(&enc.encode(&sine(440.0, 12000.0, 0)).unwrap()))
            .unwrap();
        let peak = |v: &[i16]| v.iter().map(|s| s.unsigned_abs()).max().unwrap();
        let first = peak(&dec.decode(None).unwrap());
        let mut last = first;
        for _ in 0..10 {
            last = peak(&dec.decode(None).unwrap());
        }
        assert!(first > 3000 && last < 50, "first {first}, last {last}");
    }

    #[test]
    fn rejects_malformed_payloads_and_frames() {
        let mut c = AdpcmCodec::default();
        assert_eq!(c.encode(&[0; 10]), Err(AdpcmError::WrongFrameSize));
        assert_eq!(c.decode(Some(&[0; 10])), Err(AdpcmError::WrongPayloadSize));
        let mut bad_index = vec![0u8; ENCODED_LEN];
        bad_index[2] = 200;
        assert_eq!(
            c.decode(Some(&bad_index)),
            Err(AdpcmError::WrongPayloadSize)
        );
    }
}
