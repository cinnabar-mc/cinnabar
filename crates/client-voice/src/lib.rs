//! Proximity voice chat for a Bedrock client.
//!
//! Standard library only, apart from the optional `device` feature (cpal). The session runs
//! on its own thread and is driven through [`engine::VoiceHandle`].
//!
//! ```text
//! mic -> NoiseGate -> AdpcmCodec -> VoiceClient::send_voice ==UDP==> Relay (room host)
//! Relay (filters by distance + dimension) ==UDP==> VoiceClient::poll
//!     -> JitterBuffer (per speaker) -> decode -> spatialize -> stereo mix -> speakers
//! ```

pub mod adpcm;
pub mod client;
pub mod codec;
#[cfg(feature = "device")]
pub mod device;
pub mod engine;
pub mod gate;
pub mod jitter;
pub mod proto;
pub mod relay;
pub mod room;
pub mod spatial;
