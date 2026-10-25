use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use render_model::LightmapInputs;
/// Shared classic lightmap inputs, published with the current environment frame.
#[derive(Resource, ExtractResource, Clone, Copy, Debug, Default, PartialEq)]
#[extract_app(bevy::render::RenderApp)]
pub struct WorldLighting(pub LightmapInputs);

/// A transient personal-mod override; the ordinary environment inputs remain intact.
#[derive(Resource, ExtractResource, Clone, Copy, Debug, Default, PartialEq)]
#[extract_app(bevy::render::RenderApp)]
pub struct WorldFullbright(pub bool);

/// A source selected for the vanilla renderer's small dynamic-light set.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WorldPointLightSource {
    pub position: [f32; 3],
    pub intensity: f32,
    pub radius: f32,
}

/// Experimental client-side point lights attached to light-emitting item sources.
///
/// Each source carries its own intensity and radius. RGB is linear in `[0, 1]`;
/// the vanilla renderer tints only the extra lightmap response from dynamic
/// block light, preserving existing block and sky lighting.
#[derive(Resource, ExtractResource, Clone, Copy, Debug, PartialEq)]
#[extract_app(bevy::render::RenderApp)]
pub struct WorldPointLight {
    pub enabled: bool,
    pub color: [f32; 3],
    pub local: WorldPointLightSource,
    /// Seven nearest remote-player or dropped-item sources, filling eight GPU slots total.
    pub additional: [WorldPointLightSource; 7],
}

impl Default for WorldPointLight {
    fn default() -> Self {
        Self {
            enabled: false,
            color: [1.0, 0.78, 0.52],
            local: WorldPointLightSource::default(),
            additional: [WorldPointLightSource::default(); 7],
        }
    }
}
