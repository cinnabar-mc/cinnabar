//! Physical input devices and the gameplay input mode on platforms that switch between touch and
//! keyboard-and-mouse play.

use bevy::prelude::Resource;

/// Physical input devices observed by the platform edge (e.g. Android JNI).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Resource)]
pub struct ConnectedPhysicalDevices {
    pub mice: u32,
    pub keyboards: u32,
}

/// The automatically detected gameplay input mode on platforms with dynamic input switching.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Resource)]
pub struct DetectedGameplayInputMode(pub semantic_input::InputMode);

/// The gameplay input mode driving player control and touch-control presentation.
///
/// Present only on platforms that register hybrid input; consumers treat its absence as
/// unrestricted routing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Resource)]
pub struct EffectiveGameplayInputMode(pub semantic_input::InputMode);
