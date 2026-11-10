//! Host-only native observations; old packet guests retain their original wire shape.
use serde::{Deserialize, Serialize};

/// Captured client gameplay, using actor YXZ radians: yaw left, pitch up.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeFrame {
    pub version: u32,
    pub session: u64,
    pub dimension: i32,
    pub eye: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    #[serde(default)]
    pub attack_held: bool,
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
    pub players: Vec<mod_api::native_frame::Player>,
    /// A connected world without gameplay input; only passive camera policies may be returned.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suspended: bool,
}

impl NativeFrame {
    /// Rejects unsupported or non-finite observations before crossing the guest boundary.
    pub fn is_valid(&self) -> bool {
        self.version == mod_api::native_frame::VERSION
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
            && (!self.suspended || !self.attack_held && self.players.is_empty())
            && self.players.len() <= mod_api::MAX_GAMEPLAY_PLAYERS
            && self.players.iter().all(|player| {
                player.runtime_id != 0 && player.feet.iter().all(|value| value.is_finite())
            })
    }
}

impl From<NativeFrame> for mod_api::native_frame::Frame {
    fn from(frame: NativeFrame) -> Self {
        Self {
            version: frame.version,
            session: frame.session,
            dimension: frame.dimension,
            eye: frame.eye,
            yaw: frame.yaw,
            pitch: frame.pitch,
            seconds: frame.seconds,
            look_delta: frame.look_delta,
            move_axes: frame.move_axes,
            ascend: frame.ascend,
            descend: frame.descend,
            elapsed: frame.elapsed,
            players: frame.players,
            suspended: frame.suspended,
        }
    }
}

/// Encodes the negotiated record, retaining the original JSON only for unmarked built guests.
pub(super) fn encode(frame: &Option<NativeFrame>, current: bool) -> anyhow::Result<Vec<u8>> {
    if current {
        return Ok(serde_json::to_vec(
            &frame.clone().map(mod_api::native_frame::Frame::from),
        )?);
    }
    let legacy = frame.clone().map(|mut frame| {
        frame.version = 1;
        frame
    });
    Ok(serde_json::to_vec(&legacy)?)
}

/// Admits only the explicit current native input wire marker; unmarked binaries stay legacy.
pub(super) fn current_wire(bytes: &[u8]) -> anyhow::Result<bool> {
    let binary = wat::parse_bytes(bytes)?;
    let mut current = false;
    for payload in wasmparser::Parser::new(0).parse_all(&binary) {
        if let wasmparser::Payload::CustomSection(section) = payload?
            && section.name() == mod_api::native_frame::WIRE_SECTION
        {
            anyhow::ensure!(
                !current && section.data() == [mod_api::native_frame::VERSION as u8],
                "invalid native frame wire marker"
            );
            current = true;
        }
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Current native guests decode a smaller record; built guests receive the exact old JSON.
    #[test]
    fn negotiated_wire_preserves_built_guests_without_exposing_current_holds() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../mod-api/native-frame-v1.json")).unwrap();
        let mut frame: NativeFrame = serde_json::from_value(fixture["frame"].clone()).unwrap();
        frame.version = mod_api::native_frame::VERSION;
        let legacy: serde_json::Value =
            serde_json::from_slice(&encode(&Some(frame.clone()), false).unwrap()).unwrap();
        assert_eq!(legacy, fixture["frame"]);
        let current: mod_api::native_frame::Frame =
            serde_json::from_slice(&encode(&Some(frame), true).unwrap()).unwrap();
        assert!(current.is_valid());
        let unmarked = wat::parse_str("(module)").unwrap();
        assert!(!current_wire(&unmarked).unwrap());
        let marked =
            wat::parse_str("(module (@custom \"cinnabar:extension/native-frame-wire\" \"\\02\"))")
                .unwrap();
        assert!(current_wire(&marked).unwrap());
    }
}
