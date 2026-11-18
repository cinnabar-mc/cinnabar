use bevy::prelude::Resource;

/// The barrier, light-block and structure-void marker layers shown this frame, as a set of
/// [`assets::TerrainMarker::layer`] bits. Empty hides every marker.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerrainMarkerLayers(pub u8);
