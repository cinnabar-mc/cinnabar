use super::*;
use crate::chunk::transparent::face_metric::TransparentFaceMetric;
use render_scene::{ChunkRenderInstance, ChunkTextureAssets};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MixedStream {
    Model,
    Water,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MixedFace {
    pub(crate) stream: MixedStream,
    pub(crate) index: u32,
    pub(crate) centroid: Vec3,
    pub(crate) stable: [u32; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MixedTerrainSegment {
    pub(crate) stream: MixedStream,
    pub(super) range: Range<u32>,
}

pub(super) fn merge_faces(
    key: SubChunkKey,
    camera: Vec3,
    mut faces: Vec<MixedFace>,
    segment_limit: usize,
) -> Option<Vec<MixedTerrainSegment>> {
    sort_faces(key, camera, &mut faces);
    let mut segments = Vec::<MixedTerrainSegment>::new();
    for face in faces {
        if let Some(last) = segments.last_mut()
            && last.stream == face.stream
            && last.range.end == face.index
        {
            last.range.end = face.index.checked_add(1)?;
            continue;
        }
        if segments.len() == segment_limit {
            return None;
        }
        segments.push(MixedTerrainSegment {
            stream: face.stream,
            range: face.index..face.index.checked_add(1)?,
        });
    }
    Some(segments)
}

pub(super) fn collect_faces(
    instance: &ChunkRenderInstance,
    allocation: &GpuChunkAllocation,
    model_words: &[[u32; 2]],
    assets: &ChunkTextureAssets,
    water_refs: &[PackedTransparentDrawRef],
) -> Option<Vec<MixedFace>> {
    let model_base = allocation.model_range.as_ref()?.start.checked_div(4)?;
    let liquid_base = allocation.liquid_range.as_ref()?.start.checked_div(4)?;
    let mut faces = Vec::with_capacity(model_words.len().checked_add(water_refs.len())?);
    let transparent_end = instance
        .depth_liquid_start
        .map_or(instance.liquid_quads.len(), |index| index as usize);
    for (index, draw) in water_refs.iter().enumerate() {
        if draw.metadata_index() != allocation.metadata_index {
            return None;
        }
        let local = usize::try_from(draw.liquid_record_index().checked_sub(liquid_base)?).ok()?;
        let quad = *instance.liquid_quads.get(..transparent_end)?.get(local)?;
        faces.push(MixedFace {
            stream: MixedStream::Water,
            index: u32::try_from(index).ok()?,
            centroid: Vec3::from_array(liquid_quad_centroid(instance.origin, quad)),
            stable: [u32::try_from(local).ok()?, 0],
        });
    }
    for (index, &words) in model_words.iter().enumerate() {
        let local = words[0].checked_sub(model_base)?;
        let (centroid, expected) = transparent_model_draw_candidate(
            instance.key,
            &instance.model_refs,
            &instance.model_lighting,
            PackedModelDrawRef::new(local, words[1]),
            assets.assets().model_templates(),
            assets.assets().model_quads(),
            model_base,
        )?;
        if expected != words {
            return None;
        }
        faces.push(MixedFace {
            stream: MixedStream::Model,
            index: u32::try_from(index).ok()?,
            centroid,
            stable: [local, words[1]],
        });
    }
    Some(faces)
}

/// Orders one section’s model and water faces far to near with the native metric and stable
/// ties. Each face's distance is computed once rather than in every comparison.
pub(crate) fn sort_faces(key: SubChunkKey, camera: Vec3, faces: &mut [MixedFace]) {
    let metric = TransparentFaceMetric::new(camera).for_chunk(key);
    let mut keyed: Vec<(f32, MixedFace)> = faces
        .iter()
        .map(|face| (metric.distance(face.centroid), face.clone()))
        .collect();
    keyed.sort_by(|(left_distance, left), (right_distance, right)| {
        right_distance
            .total_cmp(left_distance)
            .then_with(|| left.stream.cmp(&right.stream))
            .then_with(|| left.stable.cmp(&right.stable))
    });
    for (slot, (_, face)) in faces.iter_mut().zip(keyed) {
        *slot = face;
    }
}
