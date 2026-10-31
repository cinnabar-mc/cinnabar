//! Validated inputs for deterministic resident-render flights.

use developer_control::camera::{CameraPath, Easing, Keyframe};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Immutable inputs for one diagnostic process run; paths resolve against the plan file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Plan {
    pub(super) binary: PathBuf,
    pub(super) server_binary: PathBuf,
    pub(super) frozen_world: PathBuf,
    pub(super) user_template: PathBuf,
    pub(super) machine_id: String,
    pub(super) power: String,
    pub(super) window: [u32; 2],
    pub(super) render_distance: u32,
    pub(super) warmup_s: f32,
    pub(super) routes: Vec<Route>,
}

/// One straight, constant-speed camera flight through a fully resident fixture.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Route {
    pub(super) name: String,
    pub(super) start: [f32; 3],
    pub(super) end: [f32; 3],
    pub(super) yaw: f32,
    pub(super) pitch: f32,
    pub(super) vertical_fov: f32,
    pub(super) speed: f32,
}

impl Route {
    /// Converts distance and speed into a linear route with a stationary settling period.
    pub(super) fn path(&self, warmup: f32) -> Result<CameraPath, String> {
        let distance = self
            .start
            .iter()
            .zip(self.end)
            .map(|(a, b)| (b - a).powi(2))
            .sum::<f32>()
            .sqrt();
        let seconds = distance / self.speed;
        if !seconds.is_finite() || seconds <= 0. || !self.speed.is_finite() || self.speed <= 0. {
            return Err("route distance and speed must be finite and positive".into());
        }
        let frame = |t, position| Keyframe {
            t,
            position,
            yaw: self.yaw,
            pitch: self.pitch,
            fov: Some(self.vertical_fov),
            easing: None,
        };
        let path = CameraPath {
            keyframes: vec![
                frame(0., self.start),
                frame(warmup, self.start),
                frame(warmup + seconds, self.end),
            ],
            easing: Easing::Linear,
            looping: false,
            hide_hand: true,
        };
        path.validate()?;
        Ok(path)
    }
}
