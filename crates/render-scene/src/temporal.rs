//! Temporal upscaling requests shared by camera presentation and the renderer.

use bevy::prelude::{Component, Resource};

/// A temporal upscaler that reconstructs the output from jittered frames, depth and motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TemporalUpscaler {
    /// NVIDIA DLSS Super Resolution (DLAA at 100% render resolution).
    Dlss,
}

/// Feeds a world camera to a temporal upscaler. While present the camera is jittered, renders
/// single-sampled without SMAA, and the renderer produces motion vectors for it.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct TemporalUpscaling {
    pub upscaler: TemporalUpscaler,
    /// Changes when the camera cuts to an unrelated view; the upscaler discards its history.
    pub reset_epoch: u64,
    /// Seconds since the previous frame; long gaps discard history.
    pub delta_seconds: f32,
}

/// Which temporal upscalers the render device supports; inserted by renderer setup into both
/// the main and render worlds.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TemporalUpscalerSupport {
    pub dlss: bool,
}

impl TemporalUpscalerSupport {
    /// Whether `upscaler` can run on this device.
    #[must_use]
    pub const fn supports(self, upscaler: TemporalUpscaler) -> bool {
        match upscaler {
            TemporalUpscaler::Dlss => self.dlss,
        }
    }
}
