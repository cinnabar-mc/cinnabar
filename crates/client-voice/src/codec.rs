//! Audio frame format and the codec seam.
//!
//! The core only moves opaque encoded frames. The client plugs in Opus (the `opus` crate,
//! already a dependency of Cinnabar's server-experience crate); tests use [`TestCodec`].

/// Voice is captured, coded and mixed at 48 kHz mono in 20 ms frames.
pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAME_SAMPLES: usize = 960;

pub trait FrameCodec {
    type Error: std::fmt::Debug;

    /// Encodes exactly `FRAME_SAMPLES` samples.
    fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>, Self::Error>;

    /// Decodes a frame; `None` asks the codec to conceal a lost frame.
    fn decode(&mut self, data: Option<&[u8]>) -> Result<Vec<i16>, Self::Error>;
}

/// 8-bit companding stand-in used by tests. Lossy but deterministic.
#[derive(Debug, Default)]
pub struct TestCodec {
    last: Vec<i16>,
}

impl FrameCodec for TestCodec {
    type Error = &'static str;

    fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>, Self::Error> {
        if pcm.len() != FRAME_SAMPLES {
            return Err("wrong frame size");
        }
        Ok(pcm.iter().map(|&s| ((s >> 8) as i8) as u8).collect())
    }

    fn decode(&mut self, data: Option<&[u8]>) -> Result<Vec<i16>, Self::Error> {
        match data {
            Some(bytes) if bytes.len() == FRAME_SAMPLES => {
                self.last = bytes.iter().map(|&b| i16::from(b as i8) << 8).collect();
                Ok(self.last.clone())
            }
            Some(_) => Err("wrong payload size"),
            // Conceal by repeating the last frame at half volume.
            None => Ok(self.last.iter().map(|&s| s / 2).collect::<Vec<_>>()).map(|v| {
                if v.is_empty() {
                    vec![0; FRAME_SAMPLES]
                } else {
                    v
                }
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_within_quantization_error() {
        let mut c = TestCodec::default();
        let pcm: Vec<i16> = (0..FRAME_SAMPLES as i32)
            .map(|i| (i * 30 - 14000) as i16)
            .collect();
        let encoded = c.encode(&pcm).unwrap();
        let decoded = c.decode(Some(&encoded)).unwrap();
        assert!(
            pcm.iter()
                .zip(&decoded)
                .all(|(a, b)| (i32::from(*a) - i32::from(*b)).abs() < 256)
        );
    }

    #[test]
    fn conceals_loss_without_a_previous_frame() {
        let mut c = TestCodec::default();
        assert_eq!(c.decode(None).unwrap(), vec![0; FRAME_SAMPLES]);
    }

    #[test]
    fn rejects_wrong_sizes() {
        let mut c = TestCodec::default();
        assert!(c.encode(&[0; 10]).is_err());
        assert!(c.decode(Some(&[0; 10])).is_err());
    }
}
