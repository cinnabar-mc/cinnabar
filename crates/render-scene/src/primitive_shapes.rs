use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use render_model::primitive_shapes::PrimitiveShapeStore;
use std::sync::{Arc, Mutex};
/// Shared retained state; extraction copies only the handle and the current clock/dimension.
#[derive(Resource, Clone, ExtractResource)]
#[extract_app(bevy::render::RenderApp)]
pub struct PrimitiveShapesScene {
    pub store: Arc<Mutex<PrimitiveShapeStore>>,
    pub clock: f32,
    pub dimension: i32,
    pub render_distance: f32,
}

impl PrimitiveShapesScene {
    /// The clock rounded down to a whole hour: the only clock value the renderer uploads.
    #[must_use]
    pub fn clock_hour(&self) -> f32 {
        (self.clock / 3600.0).floor() * 3600.0
    }

    /// What the renderer reads from this scene apart from the shared store's contents: the store
    /// handle, the hour clock, the dimension and the render distance.
    #[must_use]
    pub fn published(&self) -> (*const Mutex<PrimitiveShapeStore>, u32, i32, u32) {
        (
            Arc::as_ptr(&self.store),
            self.clock_hour().to_bits(),
            self.dimension,
            self.render_distance.to_bits(),
        )
    }
}

impl Default for PrimitiveShapesScene {
    /// Starts with no geometry or GPU publications.
    fn default() -> Self {
        Self {
            store: Arc::new(Mutex::new(PrimitiveShapeStore::default())),
            clock: 0.0,
            dimension: 0,
            render_distance: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_republishes_only_at_whole_hours() {
        let mut scene = PrimitiveShapesScene {
            clock: 10.0,
            ..Default::default()
        };
        let before = scene.published();
        scene.clock = 3599.5;
        assert_eq!(
            scene.published(),
            before,
            "the renderer uploads whole hours only"
        );
        scene.clock = 3600.0;
        assert_ne!(scene.published(), before);
        let hour = scene.published();
        scene.store = Arc::new(Mutex::new(PrimitiveShapeStore::default()));
        assert_ne!(
            scene.published(),
            hour,
            "a replaced store must reach the render copy"
        );
    }
}
