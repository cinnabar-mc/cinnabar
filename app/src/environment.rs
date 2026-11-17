//! App-owned environment systems and cross-subsystem adapters.

use bevy::prelude::Resource;
use meshing::CameraMedium;

mod atmosphere;
mod diagnostics;
mod seasonal_foliage;
mod time_override;

/// Validated local weather target; authoritative weather continues to receive updates.
#[derive(Resource, Default)]
pub(crate) struct VisualWeatherOverride(pub(crate) Option<[f32; 2]>);

/// The camera mediums whose distance fog the loaded mods switched off, for the local view only.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VisualFogSuppression(pub(crate) mod_host::FogSuppression);

impl VisualFogSuppression {
    /// Whether the fog of the medium the camera is in is switched off.
    pub(crate) fn covers(self, medium: CameraMedium) -> bool {
        match medium {
            CameraMedium::Air => self.0.air,
            CameraMedium::Water => self.0.water,
            CameraMedium::Lava => self.0.lava,
        }
    }
}

/// Session-scoped environment contribution from server-offered client parts.
#[derive(Resource, Default)]
pub(crate) struct ClientPartEnvironment {
    pub(crate) time: Option<u32>,
    pub(crate) weather: Option<[f32; 2]>,
}
mod weather;

pub(crate) use atmosphere::update_atmosphere_frame;
pub(crate) use diagnostics::log_world_lighting;
pub(crate) use seasonal_foliage::update_seasonal_foliage;
pub(crate) use time_override::{FixedTime, VisualTimeOverride, rendered_clock};
pub(crate) use weather::{
    load_optional_weather_textures, update_lightning, update_precipitation_scene,
};

/// Camera-medium observation published by the app's world adapter.
#[derive(Resource, Default)]
pub(crate) struct CameraMediumState(pub(crate) CameraMedium);
