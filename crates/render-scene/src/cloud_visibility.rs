use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use meshing::{
    CLOUD_CELL_BLOCKS, CLOUD_MASK_SIZE,
    cloud_viewport::{CLOUD_FADE_START, CLOUD_REBUILD_DISTANCE_SQUARED},
};

/// The user's cloud visibility preference, copied into the render world each frame.
#[derive(Resource, ExtractResource, Clone, Copy)]
#[extract_app(bevy::render::RenderApp)]
pub struct CloudVisibility(pub bool);

impl Default for CloudVisibility {
    /// Clouds remain visible until the app supplies its saved preference.
    fn default() -> Self {
        Self(true)
    }
}

/// The user's configured cloud render distance in chunks (8..=128), copied into the render world.
#[derive(Resource, ExtractResource, Clone, Copy, Debug, PartialEq, Eq)]
#[extract_app(bevy::render::RenderApp)]
pub struct CloudDistanceChunks(pub u16);

impl CloudDistanceChunks {
    /// Returns the finite sampling span for the selected distance.
    #[must_use]
    pub fn mesh_span_cells(self) -> u16 {
        (self.0.clamp(8, 128) * 2).min(CLOUD_MASK_SIZE as u16)
    }

    /// Finishes fading before cell snapping and retained movement can expose an edge.
    #[must_use]
    pub fn fade_distance_blocks(self) -> f32 {
        let half_width = f32::from(self.mesh_span_cells()) * CLOUD_CELL_BLOCKS * 0.5;
        let guard = CLOUD_CELL_BLOCKS + CLOUD_REBUILD_DISTANCE_SQUARED.sqrt() as f32;
        (half_width - guard) / (1.0 + CLOUD_FADE_START)
    }
}

impl Default for CloudDistanceChunks {
    fn default() -> Self {
        Self(64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_distance_fade;
    use meshing::cloud_viewport::CloudViewport;

    /// Every retained boundary is transparent, including just before a cell boundary.
    #[test]
    fn configured_cloud_distance_fades_before_retained_window_edges() {
        let drift = CLOUD_REBUILD_DISTANCE_SQUARED.sqrt() - 0.000_001;
        for chunks in 8..=128 {
            let distance = CloudDistanceChunks(chunks);
            let span = distance.mesh_span_cells();
            for offset in [
                -f64::from(CLOUD_CELL_BLOCKS),
                -0.001,
                0.0,
                f64::from(CLOUD_CELL_BLOCKS) - 0.001,
            ] {
                let admitted = CloudViewport::try_new([offset; 2], span, 1, false, false).unwrap();
                let half_width = f64::from(span) * f64::from(CLOUD_CELL_BLOCKS) * 0.5;
                for movement in [[drift, 0.0], [-drift, 0.0], [0.0, drift], [0.0, -drift]] {
                    let sample = movement.map(|delta| offset + delta);
                    let requested = CloudViewport::try_new(sample, span, 1, false, false).unwrap();
                    assert!(!admitted.needs_rebuild(requested));
                    let nearest_edge = admitted
                        .centre()
                        .into_iter()
                        .zip(sample)
                        .map(|(centre, camera)| half_width - (camera - centre).abs())
                        .fold(f64::INFINITY, f64::min);
                    assert!(
                        cloud_distance_fade(nearest_edge as f32, distance.fade_distance_blocks())
                            <= f32::EPSILON,
                        "visible edge at {chunks} chunks, sample {sample:?}"
                    );
                }
            }
        }
    }
}
