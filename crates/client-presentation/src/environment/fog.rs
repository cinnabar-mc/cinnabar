mod bindings;
pub use bindings::{BiomeProfileIndex, FogBindings, FogProfileIndex};
use chunk_pipeline::{TerrainSampleCache, WorldStream};
use render_model::PRECIPITATION_SAMPLE_OFFSETS;

/// Raw biome ids on the precipitation lattice, in [`PRECIPITATION_SAMPLE_OFFSETS`] order.
pub type LatticeBiomes = [Option<u32>; PRECIPITATION_SAMPLE_OFFSETS.len()];

/// The fog and weather renderers use the same native precipitation lattice.
fn sample_positions(position: [f32; 3]) -> [[f32; 3]; PRECIPITATION_SAMPLE_OFFSETS.len()] {
    let origin = position.map(f32::floor);
    PRECIPITATION_SAMPLE_OFFSETS
        .map(|offset| std::array::from_fn(|axis| origin[axis] + offset[axis] as f32))
}

/// Reads the raw biome id at every lattice position around `position`.
pub fn lattice_biomes(stream: &WorldStream, position: [f32; 3]) -> LatticeBiomes {
    sample_positions(position).map(|position| stream.camera_biome_id(position))
}

/// The biome at the camera and on its lattice, re-read only when the camera's block, the
/// dimension or the terrain changes.
#[derive(Default)]
pub struct CameraBiomes(TerrainSampleCache<[u32; 3], (Option<u32>, LatticeBiomes)>);

impl CameraBiomes {
    /// Returns `stream.camera_biome_id(position)` and [`lattice_biomes`] at `position`.
    ///
    /// Both read only the block holding `position`, so the floored position keys the cache.
    pub fn sample(
        &mut self,
        stream: &WorldStream,
        position: [f32; 3],
    ) -> (Option<u32>, LatticeBiomes) {
        let block = position.map(f32::floor);
        self.0.sample(stream, block.map(f32::to_bits), |stream, _| {
            (stream.camera_biome_id(block), lattice_biomes(stream, block))
        })
    }
}

/// Maps the current client's 27-position biome layer to fog profiles.
pub fn fog_biome_samples(
    biomes: &LatticeBiomes,
    dimension: i32,
    bindings: &FogBindings,
) -> [Option<FogProfileIndex>; PRECIPITATION_SAMPLE_OFFSETS.len()] {
    biomes.map(|biome| bindings.fog(biome, dimension))
}

/// Counts admitted samples without renormalizing absent columns.
fn count_precipitation_samples(samples: impl IntoIterator<Item = Option<bool>>) -> Option<usize> {
    let mut known = false;
    let mut count = 0;
    for eligible in samples.into_iter().flatten() {
        known = true;
        count += usize::from(eligible);
    }
    known.then_some(count)
}

/// Counts precipitation-eligible lattice biomes in the session registry, as weather does.
pub fn fog_precipitation_count(stream: &WorldStream, biomes: &LatticeBiomes) -> Option<usize> {
    let registry = stream.authority().resolved_biome_tints();
    count_precipitation_samples(biomes.map(|id| {
        Some(world::weather::precipitation_eligible(
            registry.biome(id?)?.downfall,
        ))
    }))
}

#[cfg(test)]
mod tests;
