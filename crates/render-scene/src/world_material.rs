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

#[cfg(any(test, feature = "publication-test-support"))]
impl WorldMaterialScene {
    /// A scene binding each of `entities` to its own plain material, in order.
    pub fn with_bound_entities(entities: &[&str]) -> Self {
        use mod_render::world_material::{
            WorldMaterial,
            publication::{Blend, Depth, Published},
        };
        let material = Arc::new(WorldMaterial {
            source: "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }"
                .into(),
            textures: 0,
        });
        let mut set = WorldMaterialSet {
            revision: 1,
            ..WorldMaterialSet::default()
        };
        for (slot, entity) in entities.iter().enumerate() {
            set.materials.push(Published {
                key: slot as u64 + 1,
                material: Arc::clone(&material),
                textures: Arc::from([]),
                params: [0.0; 16],
                depth: Depth::Test,
                blend: Blend::Opaque,
            });
            set.entities.insert((*entity).into(), slot as u16);
        }
        Self { set: Arc::new(set) }
    }
}
