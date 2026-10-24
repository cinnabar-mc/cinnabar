//! Per-face order of a section's translucent world material geometry. Ordinary translucent
//! terrain sorts each section's faces back to front with the native face metric; bound blocks
//! of a translucent material use the same metric and centroids, so they are never sorted worse
//! than the Bedrock faces they replace.

use crate::chunk::transparent::face_metric::{FaceOrderClass, TransparentFaceMetric};
use crate::chunk::*;
use render_scene::ChunkRenderInstance;
use std::ops::Range;

/// The section geometry list a world material face is drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::chunk) enum WorldStream {
    Cube,
    Model,
    TransparentModel,
}

/// One tagged face: its list, its index in that list and its ordering centroid.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::chunk) struct WorldFace {
    pub(in crate::chunk) stream: WorldStream,
    pub(in crate::chunk) index: u32,
    pub(in crate::chunk) centroid: Vec3,
}

/// A run of consecutive faces of one list that draws as one instanced draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::chunk) struct WorldSegment {
    pub(in crate::chunk) stream: WorldStream,
    /// Indices into the stream's list, relative to the section's first entry.
    pub(in crate::chunk) range: Range<u32>,
}

/// What one section's face order for one material depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::chunk) struct OrderIdentity {
    pub(in crate::chunk) generation: u64,
    pub(in crate::chunk) assets: render_scene::ChunkTextureAssetIdentity,
    pub(in crate::chunk) class: FaceOrderClass,
}

/// The world-space centroid of a cube quad, the mean of the corners the cube shader emits.
pub(in crate::chunk) fn cube_quad_centroid(origin: [i32; 3], quad: meshing::PackedQuad) -> Vec3 {
    let [x, y, z] = quad.origin().map(f32::from);
    let (width, height) = (
        f32::from(quad.width()) * 0.5,
        f32::from(quad.height()) * 0.5,
    );
    let local = match quad.face() {
        meshing::Face::NegativeX => Vec3::new(x, y + height, z + width),
        meshing::Face::PositiveX => Vec3::new(x + 1.0, y + height, z + width),
        meshing::Face::NegativeY => Vec3::new(x + width, y, z + height),
        meshing::Face::PositiveY => Vec3::new(x + width, y + 1.0, z + height),
        meshing::Face::NegativeZ => Vec3::new(x + width, y + height, z),
        meshing::Face::PositiveZ => Vec3::new(x + width, y + height, z + 1.0),
    };
    Vec3::from_array(origin.map(|value| value as f32)) + local
}

/// Every face of `instance` tagged with `tag`, in list order. A model draw whose template
/// quad is missing has no centroid and is left out, as ordinary sorting leaves it out.
pub(in crate::chunk) fn tagged_faces(
    instance: &ChunkRenderInstance,
    templates: &[assets::ModelTemplate],
    model_quads: &[assets::ModelQuad],
    tag: u8,
) -> Vec<WorldFace> {
    let cubes = instance
        .cube_quads
        .iter()
        .enumerate()
        .filter(|(_, quad)| quad.world_material() == tag)
        .map(|(index, quad)| WorldFace {
            stream: WorldStream::Cube,
            index: index as u32,
            centroid: cube_quad_centroid(instance.origin, *quad),
        });
    let models = [
        (WorldStream::Model, &instance.model_draw_refs),
        (
            WorldStream::TransparentModel,
            &instance.transparent_model_draw_refs,
        ),
    ]
    .into_iter()
    .flat_map(|(stream, draws)| {
        draws
            .iter()
            .enumerate()
            .filter(|(_, draw)| draw.world_material() == tag)
            .filter_map(move |(index, draw)| {
                let (centroid, _) =
                    crate::chunk::transparent::model::transparent_model_draw_candidate(
                        instance.key,
                        &instance.model_refs,
                        &instance.model_lighting,
                        *draw,
                        templates,
                        model_quads,
                        0,
                    )?;
                Some(WorldFace {
                    stream,
                    index: index as u32,
                    centroid,
                })
            })
    });
    cubes.chain(models).collect()
}

/// `faces` back to front for `camera` with the metric ordinary translucent terrain sorts by,
/// ties broken by list and index, merged into runs of consecutive faces of one list.
pub(in crate::chunk) fn order(
    key: SubChunkKey,
    camera: Vec3,
    mut faces: Vec<WorldFace>,
) -> Vec<WorldSegment> {
    let metric = TransparentFaceMetric::new(camera).for_chunk(key);
    faces.sort_by(|left, right| {
        metric
            .distance(right.centroid)
            .total_cmp(&metric.distance(left.centroid))
            .then_with(|| left.stream.cmp(&right.stream))
            .then_with(|| left.index.cmp(&right.index))
    });
    let mut segments = Vec::<WorldSegment>::new();
    for face in faces {
        match segments.last_mut() {
            Some(last) if last.stream == face.stream && last.range.end == face.index => {
                last.range.end += 1;
            }
            _ => segments.push(WorldSegment {
                stream: face.stream,
                range: face.index..face.index + 1,
            }),
        }
    }
    segments
}

#[cfg(test)]
#[path = "order_tests.rs"]
mod tests;
