//! Real-frame camera facts consumed only by optional presentation interpolation.
use bevy::prelude::Component;

/// Removing this component stops interpolation and invalidates its history.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct FrameGenerationCamera {
    pub reset_epoch: u64,
    /// Paced real-frame period; `None` when no limit or display refresh paces real frames.
    pub interval_ns: Option<u64>,
    pub capture: bool,
    pub near: f32,
    pub far: f32,
    pub fov_y: f32,
}

/// A complete native overlay and interpolation context was admitted for this render view.
#[derive(Component)]
pub struct FrameGenerationActive;
