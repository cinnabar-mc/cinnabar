//! Uses ordinary terrain's face metric and reconstruction for one batched transparent stream.
use super::*;
use crate::chunk::transparent::mixed::plan::{MixedFace, MixedStream, sort_faces};
use meshing::liquid::{TRANSPARENT_WATER_DRAW_FLAG, TRANSPARENT_WATER_RECORD_FLAG};

pub(super) struct Face {
    pub key: SubChunkKey,
    pub centroid: Vec3,
    pub stream: MixedStream,
    pub stable: [u32; 2],
    pub reference: [u32; 2],
}
pub(super) struct Order {
    pub _buffer: mod_render::GpuAllocation<Buffer>,
    pub count: u32,
    pub groups: BTreeMap<SubChunkKey, Range<u32>>,
}

/// Computes native face centroids once, after the ordinary uploader has assigned addresses.
pub(super) fn faces(
    instances: &[ChunkRenderInstance],
    native: &PreparedResourceGeometry,
    assets: &RuntimeAssets,
) -> Option<Arc<[Face]>> {
    let by_key: HashMap<_, _> = instances.iter().map(|i| (i.key, i)).collect();
    let mut faces = Vec::new();
    let mut allocations: Vec<_> = native.arena.allocations.values().collect();
    allocations.sort_by_key(|allocation| allocation.gpu.key);
    for allocation in allocations {
        let gpu = &allocation.gpu;
        let instance = by_key.get(&gpu.key)?;
        if let (Some(draws), Some(models)) = (&gpu.transparent_model_draw_range, &gpu.model_range) {
            for (index, draw) in instance.transparent_model_draw_refs.iter().enumerate() {
                let (centroid, _) = transparent_model_draw_candidate(
                    gpu.key,
                    &instance.model_refs,
                    &instance.model_lighting,
                    *draw,
                    assets.model_templates(),
                    assets.model_quads(),
                    models.start / 4,
                )?;
                faces.push(Face {
                    key: gpu.key,
                    centroid,
                    stream: MixedStream::Model,
                    stable: draw.words(),
                    reference: [draws.start / 2 + index as u32, gpu.metadata_index * 4],
                });
            }
        }
        if let Some(liquids) = &gpu.liquid_range {
            for (index, quad) in instance
                .liquid_quads
                .iter()
                .take_while(|q| !q.is_depth_writing())
                .enumerate()
            {
                faces.push(Face {
                    key: gpu.key,
                    centroid: Vec3::from_array(liquid_quad_centroid(instance.origin, *quad)),
                    stream: MixedStream::Water,
                    stable: [index as u32, 0],
                    reference: [
                        (liquids.start / 4 + index as u32)
                            | TRANSPARENT_WATER_DRAW_FLAG
                            | TRANSPARENT_WATER_RECORD_FLAG,
                        0,
                    ],
                });
            }
        }
    }
    Some(faces.into())
}

/// Sorts outside the frame thread; unchanged camera positions never schedule it again.
pub(super) fn sort(
    native: Arc<Native>,
    camera: Vec3,
    direction: Vec3,
    device: RenderDevice,
    queue: RenderQueue,
    ledger: &mod_render::GpuLedger,
) -> Option<Order> {
    let (refs, groups) = references(&native.faces, camera, direction);
    let count = refs.len() as u32;
    let bytes = refs.len().max(1) * std::mem::size_of::<[u32; 2]>();
    let buffer = ledger.allocate(bytes as u64, || {
        crate::gpu_allocation::buffer_with_data(
            &device,
            &queue,
            "block transparent order",
            if refs.is_empty() {
                bytemuck::cast_slice(&[[0u32; 2]])
            } else {
                bytemuck::cast_slice(&refs)
            },
            BufferUsages::STORAGE,
        )
    })?;
    Some(Order {
        _buffer: buffer,
        count,
        groups,
    })
}

/// Orders sections by view depth and delegates their mixed face order to ordinary terrain.
fn references(
    faces: &[Face],
    camera: Vec3,
    direction: Vec3,
) -> (Vec<[u32; 2]>, BTreeMap<SubChunkKey, Range<u32>>) {
    let mut groups = BTreeMap::<SubChunkKey, Vec<MixedFace>>::new();
    for (index, face) in faces.iter().enumerate() {
        groups.entry(face.key).or_default().push(MixedFace {
            stream: face.stream,
            index: index as u32,
            centroid: face.centroid,
            stable: face.stable,
        });
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by(|(left, _), (right, _)| {
        let depth = |key| {
            let center = Vec3::from_array(chunk_origin(key).map(|v| v as f32)) + Vec3::splat(8.);
            (center - camera).dot(direction)
        };
        depth(*right)
            .total_cmp(&depth(*left))
            .then_with(|| left.cmp(right))
    });
    let mut refs = Vec::with_capacity(faces.len());
    let mut ranges = BTreeMap::new();
    for (key, mut group) in groups {
        let start = refs.len() as u32;
        sort_faces(key, camera, &mut group);
        refs.extend(
            group
                .into_iter()
                .map(|face| faces[face.index as usize].reference),
        );
        ranges.insert(key, start..refs.len() as u32);
    }
    (refs, ranges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tied_section_depths_keep_sections_together_and_native_mixed_ties() {
        let face = |x, stream, id| Face {
            key: SubChunkKey::new(0, x, 0, 0),
            centroid: Vec3::new(x as f32 * 16. + 1., 1., 1.),
            stream,
            stable: [0, 0],
            reference: [id, 0],
        };
        let faces = [
            face(1, MixedStream::Water, 3),
            face(0, MixedStream::Water, 2),
            face(1, MixedStream::Model, 4),
            face(0, MixedStream::Model, 1),
        ];
        assert_eq!(
            references(&faces, Vec3::ZERO, Vec3::NEG_Z).0,
            [1, 2, 4, 3].map(|id| [id, 0])
        );
    }
}
