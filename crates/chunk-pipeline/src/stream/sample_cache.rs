//! Reuses a camera sample while its query and everything it reads from the stream are unchanged.

use std::sync::Arc;

use super::WorldStream;

/// What a terrain sample reads besides its query: the stored terrain, the dimension, the block
/// assets and the meshing policy.
struct SampleSource {
    terrain: u64,
    dimension: i32,
    mesh_render: u64,
    /// Held so a replacement can never reuse its address.
    assets: Arc<assets::RuntimeAssets>,
}

impl SampleSource {
    fn of(stream: &WorldStream) -> Self {
        Self {
            terrain: stream.collision_store().content_revision(),
            dimension: stream.current_dimension(),
            mesh_render: stream.mesh_render_revision(),
            assets: Arc::clone(stream.runtime_assets()),
        }
    }

    fn is_current(&self, stream: &WorldStream) -> bool {
        self.terrain == stream.collision_store().content_revision()
            && self.dimension == stream.current_dimension()
            && self.mesh_render == stream.mesh_render_revision()
            && Arc::ptr_eq(&self.assets, stream.runtime_assets())
    }
}

/// The last value of one stream sample, such as the biome or medium at the camera.
///
/// [`Self::sample`] returns the cached value while the query is equal and the stream's
/// terrain, dimension, block assets and meshing policy are unchanged, so it returns exactly
/// what sampling again would. Queries should compare positions bit for bit.
pub struct TerrainSampleCache<Q, V> {
    last: Option<(SampleSource, Q, V)>,
}

impl<Q, V> Default for TerrainSampleCache<Q, V> {
    fn default() -> Self {
        Self { last: None }
    }
}

impl<Q: PartialEq, V: Clone> TerrainSampleCache<Q, V> {
    /// Returns `sample(stream, &query)`, reusing the last result while nothing it reads changed.
    pub fn sample(
        &mut self,
        stream: &WorldStream,
        query: Q,
        sample: impl FnOnce(&WorldStream, &Q) -> V,
    ) -> V {
        if let Some((source, last, value)) = &self.last
            && *last == query
            && source.is_current(stream)
        {
            return value.clone();
        }
        let value = sample(stream, &query);
        self.last = Some((SampleSource::of(stream), query, value.clone()));
        value
    }
}
