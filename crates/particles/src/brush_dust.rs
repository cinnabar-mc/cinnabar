//! The dust a brush sweeps off the block it is used on.
//!
//! Vanilla's client draws it as legacy particle 86 (brush dust), which goes through the
//! level's terrain particle effect like a block break, so it is the block destruction
//! effect textured and tinted from the brushed block.

use super::{
    emitter::{SpawnRequest, TileRequest},
    triggers::block_break_request,
};

/// Terrain effect intensity, velocity scale and emitter radius for dust swept off a
/// block's top face.
const TOP_FACE_DUST: [f32; 3] = [2.376, 0.971, 0.325];
/// The same arguments for dust swept off any other face.
const SIDE_FACE_DUST: [f32; 3] = [1.987, 0.662, 0.183];
/// The protocol face index of a block's top face.
const TOP_FACE: u8 = 1;
/// Below this intensity a terrain effect's particle count is the intensity cubed.
const TERRAIN_CUBIC_INTENSITY_LIMIT: f32 = 2.0;
/// The most particles a terrain effect's intensity asks for.
const TERRAIN_MAX_PARTICLES: f32 = 255.0;

/// The particle count the level's terrain effect draws for `intensity`: its cube below 2,
/// and `(intensity - 1) * intensity²` clamped to 0..=255 from there.
#[must_use]
pub(crate) fn terrain_effect_particles(intensity: f32) -> f32 {
    if intensity < TERRAIN_CUBIC_INTENSITY_LIMIT {
        return intensity.powi(3);
    }
    ((intensity - 1.0) * intensity * intensity).clamp(0.0, TERRAIN_MAX_PARTICLES)
}

/// Dust for one brush stroke on `face` of `block`, emitted at the stroke's world-space
/// `hit` and textured with the brushed block's `tile` and `tint`.
#[must_use]
pub fn brush_dust_request(
    effect: &str,
    block: [i32; 3],
    face: u8,
    hit: [f32; 3],
    tile: TileRequest,
    tint: [f32; 4],
) -> SpawnRequest {
    let [intensity, velocity, radius] = if face == TOP_FACE {
        TOP_FACE_DUST
    } else {
        SIDE_FACE_DUST
    };
    let particles = terrain_effect_particles(intensity);
    let mut request = block_break_request(effect, block, tile, tint);
    request.position = hit;
    for (name, value) in &mut request.variables {
        *value = match name.as_str() {
            "emitter_particles_count" => particles,
            "emitter_intensity" => particles.cbrt(),
            "emitter_radius" => radius,
            "velocity_scalar" => velocity,
            _ => continue,
        };
    }
    request
}

#[cfg(test)]
#[path = "brush_dust_tests.rs"]
mod tests;
