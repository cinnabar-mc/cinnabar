//! One back-to-front face order per section for its translucent world material faces and its
//! ordinary translucent faces. Ordinary translucent terrain sorts each section's water and
//! model faces with the native face metric; a section with bound translucent blocks merges those
//! blocks' faces into the same order, so a bound face and the water or glass beside it blend in
//! the order Bedrock would draw two ordinary faces.

use crate::chunk::transparent::face_metric::{FaceOrderClass, TransparentFaceMetric};
use crate::chunk::*;
use render_scene::ChunkRenderInstance;
use std::ops::Range;

/// The section geometry list a world material face is drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::chunk) enum WorldList {
    Cube,
    Model,
    TransparentModel,
}

/// The pipeline and list one face draws with: an ordinary translucent model draw or water
/// record, or a world material face of one slot. Ties between equally distant faces break in
/// this order, models before water as ordinary sorting breaks them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::chunk) enum Stream {
    Model,
    Water,
    World { slot: u16, list: WorldList },
}

/// One face: its stream, its index in that stream's list as the GPU holds it, and its
/// ordering centroid.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::chunk) struct Face {
    pub(in crate::chunk) stream: Stream,
    pub(in crate::chunk) index: u32,
    pub(in crate::chunk) centroid: Vec3,
}

/// A run of consecutive faces of one stream that draws as one instanced draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::chunk) struct Segment {
    pub(in crate::chunk) stream: Stream,
    /// Indices into the stream's list, relative to the section's first entry.
    pub(in crate::chunk) range: Range<u32>,
}

/// What one section's merged order depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::chunk) struct OrderIdentity {
    pub(in crate::chunk) generation: u64,
    pub(in crate::chunk) assets: render_scene::ChunkTextureAssetIdentity,
    pub(in crate::chunk) class: FaceOrderClass,
    /// The revision of the uploaded translucent model order the model indices name; `None`
    /// while the GPU still holds the mesher's order.
    pub(in crate::chunk) model_revision: Option<u64>,
    /// The translucent material slots the order includes.
    pub(in crate::chunk) slots: u64,
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

/// The stream of a face tagged `tag` in `list`: a world material face when its slot is in
/// `slots`, `None` for a tag of another slot, whose material draws in the opaque phase.
fn world_stream(tag: u8, slots: u64, list: WorldList) -> Option<Stream> {
    let slot = u16::from(tag.checked_sub(1)?);
    (slots & (1 << slot) != 0).then_some(Stream::World { slot, list })
}

/// The section's inputs to one merged order.
pub(in crate::chunk) struct SectionFaces<'a> {
    pub(in crate::chunk) instance: &'a ChunkRenderInstance,
    /// The translucent model draw refs in the order the GPU holds them, with absolute model
    /// ref words, and the section's first model ref.
    pub(in crate::chunk) model_words: &'a [[u32; 2]],
    pub(in crate::chunk) model_base: u32,
    pub(in crate::chunk) templates: &'a [assets::ModelTemplate],
    pub(in crate::chunk) model_quads: &'a [assets::ModelQuad],
    /// Translucent world material slots, one bit each; their tagged faces join the order.
    pub(in crate::chunk) slots: u64,
}

/// Every face the merged order draws: untagged translucent model draws and transparent water
/// records, and the faces of `slots` from every list. A model draw whose template quad is
/// missing has no centroid and is left out, as ordinary sorting leaves it out.
pub(in crate::chunk) fn collect_faces(section: &SectionFaces<'_>) -> Vec<Face> {
    let instance = section.instance;
    let mut faces = Vec::new();
    let model_centroid = |draw: meshing::PackedModelDrawRef| {
        crate::chunk::transparent::model::transparent_model_draw_candidate(
            instance.key,
            &instance.model_refs,
            &instance.model_lighting,
            draw,
            section.templates,
            section.model_quads,
            0,
        )
        .map(|(centroid, _)| centroid)
    };
    for (index, words) in section.model_words.iter().enumerate() {
        let Some(local) = words[0].checked_sub(section.model_base) else {
            continue;
        };
        let draw = meshing::PackedModelDrawRef::new(local, words[1]);
        let stream = match draw.world_material() {
            0 => Some(Stream::Model),
            tag => world_stream(tag, section.slots, WorldList::TransparentModel),
        };
        if let (Some(stream), Some(centroid)) = (stream, model_centroid(draw)) {
            faces.push(Face {
                stream,
                index: index as u32,
                centroid,
            });
        }
    }
    for (index, draw) in instance.model_draw_refs.iter().enumerate() {
        if let (Some(stream), Some(centroid)) = (
            world_stream(draw.world_material(), section.slots, WorldList::Model),
            model_centroid(*draw),
        ) {
            faces.push(Face {
                stream,
                index: index as u32,
                centroid,
            });
        }
    }
    for (index, quad) in instance.cube_quads.iter().enumerate() {
        if let Some(stream) = world_stream(quad.world_material(), section.slots, WorldList::Cube) {
            faces.push(Face {
                stream,
                index: index as u32,
                centroid: cube_quad_centroid(instance.origin, *quad),
            });
        }
    }
    let transparent_water = instance
        .depth_liquid_start
        .map_or(instance.liquid_quads.len(), |start| start as usize);
    for (index, quad) in instance.liquid_quads[..transparent_water.min(instance.liquid_quads.len())]
        .iter()
        .enumerate()
    {
        faces.push(Face {
            stream: Stream::Water,
            index: index as u32,
            centroid: Vec3::from_array(liquid_quad_centroid(instance.origin, *quad)),
        });
    }
    faces
}

/// `faces` back to front for `camera` with the metric ordinary translucent terrain sorts by,
/// ties broken by stream and index, merged into runs of consecutive faces of one stream.
pub(in crate::chunk) fn order(
    key: SubChunkKey,
    camera: Vec3,
    mut faces: Vec<Face>,
) -> Vec<Segment> {
    let metric = TransparentFaceMetric::new(camera).for_chunk(key);
    faces.sort_by(|left, right| {
        metric
            .distance(right.centroid)
            .total_cmp(&metric.distance(left.centroid))
            .then_with(|| left.stream.cmp(&right.stream))
            .then_with(|| left.index.cmp(&right.index))
    });
    let mut segments = Vec::<Segment>::new();
    for face in faces {
        match segments.last_mut() {
            Some(last) if last.stream == face.stream && last.range.end == face.index => {
                last.range.end += 1;
            }
            _ => segments.push(Segment {
                stream: face.stream,
                range: face.index..face.index + 1,
            }),
        }
    }
    segments
}

#[cfg(test)]
#[path = "interleave_tests.rs"]
mod tests;
