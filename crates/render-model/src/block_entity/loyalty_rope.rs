//! The rope a returning loyalty trident draws back to its owner: two textured triangle
//! strips that sag and coil along the span, as vanilla's thrown-trident renderer draws them.

use glam::Vec3;

use super::{
    atlas::{AtlasRect, BlockEntityAtlas},
    mesh::{BlockEntityVertex, MAX_BLOCK_ENTITY_VERTICES, MeshBuilder},
};
use crate::java_animation::{java_cos, java_sin};

/// Segments across the whole span; a rope this long is drawn once fully extended.
const SEGMENTS: u32 = 37;
/// Segments per repeat of the texture along the rope.
const TEXTURE_REPEAT: u32 = 7;
/// Span length in blocks at which the sag reaches its cap.
const SAG_LENGTH: f32 = 30.0;
const MAX_SAG: f32 = 0.5;
/// Coil turns over the span, in half turns.
const COIL_HALF_TURNS: f32 = 8.0;
/// Height of the rope above the trident-to-owner line before any sag.
const LIFT: f64 = 0.1;
/// Each strip's vertical ripple, which keeps the two strips apart.
const RIPPLES: [f32; 2] = [0.02, 0.01];
/// Strip width as a fraction of the local sag.
const WIDTH_PER_SAG: f32 = 0.1;

/// One loyalty rope frame, in world blocks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoyaltyRopeModel {
    /// Interpolated trident position, where the rope starts.
    pub trident: [f32; 3],
    /// Offset from the trident to the rope's owner end.
    pub span: [f32; 3],
    /// Owner yaw in degrees, which turns the coil.
    pub owner_yaw_degrees: f32,
    /// Coil phase in radians at the trident end.
    pub phase: f32,
    /// Ticks the trident has been returning; the rope grows two segments a tick.
    pub return_ticks: u32,
}

/// Emits both strips, each a pair of vertices per drawn segment boundary.
pub(super) fn emit(builder: &mut MeshBuilder, atlas: &BlockEntityAtlas, model: &LoyaltyRopeModel) {
    let finite = model
        .trident
        .iter()
        .chain(&model.span)
        .chain(&[model.owner_yaw_degrees, model.phase])
        .all(|value| value.is_finite());
    if !finite {
        return;
    }
    let Some(texture) = atlas.texture(assets::LOYALTY_ROPE_TEXTURE, [1.0; 2]) else {
        return;
    };
    let span = Vec3::from_array(model.span);
    let sag_scale = (span.length() / SAG_LENGTH).min(MAX_SAG);
    let yaw = model.owner_yaw_degrees.to_radians();
    let (sine, cosine) = (java_sin(yaw), java_cos(yaw));
    let last = model
        .return_ticks
        .saturating_mul(2)
        .saturating_add(1)
        .min(SEGMENTS);
    for (strip, ripple) in RIPPLES.into_iter().enumerate() {
        let pairs = (0..=last).map(|segment| {
            let t = segment as f32 / SEGMENTS as f32;
            let u = ((segment % TEXTURE_REPEAT) as f32 / TEXTURE_REPEAT as f32).clamp(0.0, 1.0);
            let centered = t + t - 1.0;
            let sag = (1.0 - centered * centered) * sag_scale;
            let coil = t * std::f32::consts::PI * COIL_HALF_TURNS + model.phase;
            let along = span * t + Vec3::from_array(model.trident);
            let position = Vec3::new(
                (cosine - sine) * java_sin(coil) * sag + along.x,
                (f64::from(sag) + LIFT + f64::from(java_cos(coil) * ripple + along.y)) as f32,
                java_sin(coil) * (cosine + sine) * sag + along.z,
            );
            let width = sag * WIDTH_PER_SAG;
            let edge = if strip == 0 {
                Vec3::new(width, width, 0.0)
            } else {
                Vec3::new(width, 0.0, width)
            };
            [(position, u), (position + edge, u)]
        });
        emit_strip(builder, atlas, texture.rect, pairs);
    }
}

/// Triangulates a strip of vertex pairs, keeping vanilla's strip order.
fn emit_strip(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    rect: AtlasRect,
    pairs: impl Iterator<Item = [(Vec3, f32); 2]>,
) {
    let size = atlas.size().map(|side| side as f32);
    let shade = builder.light;
    let actor_light = builder.actor_light;
    // Vanilla samples the rope's bottom edge; the centre of its last texel row stays inside
    // this placement in the shared atlas.
    let v = rect.y + rect.height - 0.5;
    let vertex = |(position, u): (Vec3, f32)| BlockEntityVertex {
        position: position.to_array(),
        uv: [(rect.x + rect.width * u) / size[0], v / size[1]],
        color: [shade, shade, shade, 1.0],
        normal: Vec3::Y.to_array(),
        actor_light,
    };
    let mut previous: Option<[(Vec3, f32); 2]> = None;
    for pair in pairs {
        if let Some([a, b]) = previous {
            if builder.solid.len() + 6 > MAX_BLOCK_ENTITY_VERTICES {
                builder.rejected_quads = builder.rejected_quads.saturating_add(1);
                return;
            }
            let [c, d] = pair;
            builder.solid.extend([a, b, c, c, b, d].map(vertex));
        }
        previous = Some(pair);
    }
}

#[cfg(test)]
#[path = "loyalty_rope_tests.rs"]
mod tests;
