//! Voice-activity noise gate with hysteresis and a hang time, so words are not clipped
//! at the end and fans or keyboards are not transmitted.

#[derive(Debug, Clone)]
pub struct NoiseGate {
    open_threshold: f32,
    close_threshold: f32,
    hang_frames: u32,
    remaining: u32,
    open: bool,
}

impl NoiseGate {
    /// Thresholds are RMS levels in `[0, 1]`; `hang_frames` counts 20 ms frames.
    pub fn new(open_threshold: f32, close_threshold: f32, hang_frames: u32) -> Self {
        assert!(close_threshold <= open_threshold);
        Self {
            open_threshold,
            close_threshold,
            hang_frames,
            remaining: 0,
            open: false,
        }
    }

    /// Reasonable defaults: opens at about -38 dBFS, closes at -44 dBFS, 200 ms hang.
    pub fn default_voice() -> Self {
        Self::new(0.0125, 0.0063, 10)
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Returns true if this frame should be transmitted.
    pub fn process(&mut self, frame: &[i16]) -> bool {
        let level = rms(frame);
        if level >= self.open_threshold {
            self.open = true;
            self.remaining = self.hang_frames;
        } else if self.open && level >= self.close_threshold {
            self.remaining = self.hang_frames;
        } else if self.open {
            if self.remaining == 0 {
                self.open = false;
            } else {
                self.remaining -= 1;
            }
        }
        self.open
    }
}

/// Root-mean-square level of 16-bit PCM, normalized to `[0, 1]`.
pub fn rms(frame: &[i16]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let sum: f64 = frame
        .iter()
        .map(|&s| (f64::from(s) / 32768.0).powi(2))
        .sum();
    (sum / frame.len() as f64).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(amplitude: i16) -> Vec<i16> {
        (0..960)
            .map(|i| if i % 2 == 0 { amplitude } else { -amplitude })
            .collect()
    }

    #[test]
    fn rms_of_silence_and_full_scale() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&[0; 960]), 0.0);
        assert!((rms(&tone(i16::MAX)) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn stays_closed_for_background_noise() {
        let mut g = NoiseGate::default_voice();
        for _ in 0..50 {
            assert!(!g.process(&tone(30)));
        }
    }

    #[test]
    fn opens_on_speech_and_holds_through_the_tail() {
        let mut g = NoiseGate::new(0.05, 0.02, 3);
        assert!(g.process(&tone(5000)));
        let quiet = tone(10);
        // Three frames of hang time keep the gate open, then it closes.
        assert!(g.process(&quiet));
        assert!(g.process(&quiet));
        assert!(g.process(&quiet));
        assert!(!g.process(&quiet));
        assert!(!g.is_open());
    }

    #[test]
    fn hysteresis_keeps_it_open_between_the_thresholds() {
        let mut g = NoiseGate::new(0.10, 0.02, 0);
        assert!(g.process(&tone(8000)));
        for _ in 0..20 {
            assert!(g.process(&tone(1500))); // above close threshold, below open threshold
        }
    }
}
