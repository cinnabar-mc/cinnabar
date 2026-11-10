//! Versioned native frame wire contract for trusted WASI packet mods.

use serde::{Deserialize, Serialize};

// One declaration supplies the public wire constants and the reactor's literal section attribute.
macro_rules! wire_contract {
    ($version:literal, $section:literal) => {
        /// JSON contract version, independent of the packet relay's existing WIT version.
        /// Built guests without the current wire marker retain their original record.
        pub const VERSION: u32 = $version;
        /// Custom section selecting the current native frame wire, containing its version byte.
        pub const WIRE_SECTION: &str = $section;

        /// Selects the current native frame JSON contract for a native packet reactor.
        /// Put this invocation at module scope when decoding `native_frame::Frame`.
        #[macro_export]
        macro_rules! current_native_frames {
            () => {
                #[used]
                #[unsafe(link_section = $section)]
                static CINNABAR_NATIVE_FRAME_WIRE: [u8; 1] = [$version];
            };
        }
    };
}
wire_contract!(2, "cinnabar:extension/native-frame-wire");
/// A host can supply input-owned player frames and accept local camera deltas.
pub const CAMERA_PLAYERS: u32 = 1;
/// Maximum JSON input or output bytes in one native frame callback.
pub const MAX_BYTES: usize = 64 * 1024;

/// One nearby remote player's runtime identity and feet position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Player {
    pub runtime_id: u64,
    pub feet: [f32; 3],
}

/// Captured client gameplay, using actor YXZ radians: yaw left, pitch up.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub version: u32,
    pub session: u64,
    pub dimension: i32,
    pub eye: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub seconds: f32,
    /// Look the host applied to the captured view since the last delivered frame, as yaw and
    /// pitch radians; zero without `capture_look`.
    pub look_delta: [f32; 2],
    /// The ordinary movement bindings, x right and y forward, each -1 to 1.
    pub move_axes: [f32; 2],
    /// The jump binding is held.
    pub ascend: bool,
    /// The sneak binding is held.
    pub descend: bool,
    /// Real seconds since the previous delivered frame; 0 on the first frame after a reset.
    pub elapsed: f32,
    pub players: Vec<Player>,
    /// A connected world without gameplay input; only passive camera policies may be returned.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suspended: bool,
}

impl Frame {
    /// Rejects unsupported or non-finite observations before crossing the guest boundary.
    pub fn is_valid(&self) -> bool {
        self.version == VERSION
            && self.eye.iter().all(|value| value.is_finite())
            && self.yaw.is_finite()
            && self.pitch.is_finite()
            && self.seconds.is_finite()
            && (0.0..=1.0).contains(&self.seconds)
            && self.elapsed.is_finite()
            && self.elapsed >= 0.0
            && self.look_delta.iter().all(|value| value.is_finite())
            && self
                .move_axes
                .iter()
                .all(|value| value.is_finite() && (-1.0..=1.0).contains(value))
            && (!self.suspended || self.players.is_empty())
            && self.players.len() <= crate::MAX_GAMEPLAY_PLAYERS
            && self.players.iter().all(|player| {
                player.runtime_id != 0 && player.feet.iter().all(|value| value.is_finite())
            })
    }
}

/// Once-consumed local rotation; an absent output leaves the camera untouched.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delta {
    pub yaw: f32,
    pub pitch: f32,
}

impl Delta {
    /// Uses the same finite accumulated camera bounds as regular player mods.
    pub fn is_valid(self) -> bool {
        self.yaw.is_finite()
            && self.pitch.is_finite()
            && self.yaw.abs() <= crate::MAX_CAMERA_DELTA_RADIANS
            && self.pitch.abs() <= crate::MAX_CAMERA_DELTA_RADIANS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_wire_fixture_matches_the_sdk() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../native-frame-v2.json")).unwrap();
        assert_eq!(fixture["version"], VERSION);
        assert_eq!(fixture["capabilities"], CAMERA_PLAYERS);
        assert_eq!(fixture["max_bytes"], MAX_BYTES);
        assert_eq!(fixture["max_players"], crate::MAX_GAMEPLAY_PLAYERS);
        assert_eq!(
            fixture["max_delta_radians"],
            crate::MAX_CAMERA_DELTA_RADIANS
        );
        let frame: Frame = serde_json::from_value(fixture["frame"].clone()).unwrap();
        assert!(frame.is_valid());
        let delta: Delta = serde_json::from_value(fixture["delta"].clone()).unwrap();
        assert!(delta.is_valid());
        assert_eq!(serde_json::to_value(frame).unwrap(), fixture["frame"]);
        assert_eq!(serde_json::to_value(delta).unwrap(), fixture["delta"]);
    }
}
