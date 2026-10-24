//! The session's programmable world materials, as the main world publishes them to the
//! renderer.

use bevy::prelude::Resource;
use mod_render::world_material::publication::WorldMaterialSet;
use std::sync::Arc;

/// The current world material set; replaced only when it changes, so an unchanged session
/// extracts the same allocation.
#[derive(Resource, bevy::render::extract_resource::ExtractResource, Clone, Default)]
#[extract_app(bevy::render::RenderApp)]
pub struct WorldMaterialScene {
    pub set: Arc<WorldMaterialSet>,
}
