use assets::{NetworkIdMode, RuntimeAssets};
use world::{BLOCKS_PER_SUB_CHUNK, SubChunk};

use crate::{
    ChunkMesh, DiagnosticGeometryCount, Face, MAX_DIAGNOSTIC_IDENTITIES_PER_MESH,
    MAX_PACKED_BIOME_RECORD_WORDS, PackedBiomeRecord, PackedLiquidQuad, PackedModelDrawRef,
    PackedModelRef, PackedQuad, PackedQuadLighting,
    chunk::models::{
        MAX_SELECTED_MODEL_TEMPLATES, ModelTemplateSelection, model_template_selection,
    },
    types::{CubeStreams, ModelDrawRefs},
};

/// Conservative packed-output sizes for one immutable runtime asset set.
#[derive(Debug)]
pub struct MeshOutputBounds {
    model_templates: Box<[u64]>,
    fallback_model: u64,
    surface_extra: Box<[u64]>,
    fallback_surface_extra: u64,
}

impl MeshOutputBounds {
    /// Accounts for every cell, face, selected model and compound model part.
    #[must_use]
    pub fn new(assets: &RuntimeAssets) -> Self {
        let quads = assets
            .model_templates()
            .iter()
            .map(|t| u64::from(t.quad_count))
            .max()
            .unwrap_or(0);
        let offset_prefix = assets
            .model_templates()
            .iter()
            .enumerate()
            .any(|(id, _)| assets.model_random_offset(id as u32).is_some());
        let part = model_part_bound(quads, offset_prefix);
        let mut chain = 0_u64;
        let mut max_parts = 1_u64;
        for template in assets.model_templates() {
            chain += 1;
            max_parts = max_parts.max(chain);
            if template.flags & assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT == 0 {
                chain = 0;
            }
        }
        let fallback_model = max_parts * part;
        let mut model_templates = vec![0; assets.model_templates().len()];
        let mut chain_bytes = 0;
        let mut surface_extra = vec![0; model_templates.len()];
        let mut chain_extra = 0;
        for (index, template) in assets.model_templates().iter().enumerate().rev() {
            let continuation = if template.flags & assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT != 0 {
                chain_bytes
            } else {
                0
            };
            chain_bytes =
                model_part_bound(u64::from(template.quad_count), offset_prefix) + continuation;
            model_templates[index] = chain_bytes;
            let continuation = if template.flags & assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT != 0 {
                chain_extra
            } else {
                0
            };
            let faces = assets.model_quads()[template.quad_start as usize..]
                [..template.quad_count as usize]
                .iter()
                .filter(|quad| {
                    crate::surface::full_face(quad).is_some()
                        && quad.material != assets::DIAGNOSTIC_MATERIAL
                        && assets.material(quad.material).flags
                            & (assets::MATERIAL_FLAG_ALPHA_BLEND
                                | assets::MATERIAL_FLAG_ALPHA_CUTOUT)
                            != 0
                })
                .count() as u64;
            chain_extra = continuation + faces * 7 * bytes::<PackedModelDrawRef>(1);
            surface_extra[index] = chain_extra;
        }
        Self {
            fallback_surface_extra: surface_extra.iter().copied().max().unwrap_or(0),
            surface_extra: surface_extra.into_boxed_slice(),
            model_templates: model_templates.into_boxed_slice(),
            fallback_model,
        }
    }

    /// Reserves the largest model the source palette can emit, including selected alternatives.
    #[must_use]
    pub fn for_sub_chunk(
        &self,
        source: &SubChunk,
        assets: &RuntimeAssets,
        mode: NetworkIdMode,
    ) -> u64 {
        self.for_sub_chunk_with_surface_patches(source, assets, mode, &[])
    }

    /// Includes at most eight retained patches per model face without inflating cube or liquid storage.
    #[must_use]
    pub fn for_sub_chunk_with_surface_patches(
        &self,
        source: &SubChunk,
        assets: &RuntimeAssets,
        mode: NetworkIdMode,
        rules: &[crate::surface::SurfaceRule],
    ) -> u64 {
        let model_bytes = source
            .storages()
            .iter()
            .flat_map(|storage| storage.palette().values())
            .filter_map(|&id| {
                let visual = assets.resolve(mode, id);
                let index = visual.model_template()? as usize;
                let selection = assets
                    .model_templates()
                    .get(index)
                    .map(|template| model_template_selection(visual.kind(), template.flags));
                let bound = match (self.model_templates.get(index), selection) {
                    (Some(&bound), Some(ModelTemplateSelection::Fixed)) => bound,
                    _ => self.fallback_model,
                };
                let extra = if !visual
                    .flags()
                    .contains(assets::BlockFlags::OCCLUDES_FULL_FACE)
                    && rules
                        .iter()
                        .any(|rule| rule.states.binary_search(&id).is_ok())
                {
                    match selection {
                        Some(ModelTemplateSelection::Fixed) => self
                            .surface_extra
                            .get(index)
                            .copied()
                            .unwrap_or(self.fallback_surface_extra),
                        _ => self.fallback_surface_extra,
                    }
                } else {
                    0
                };
                Some(bound + extra)
            })
            .max()
            .unwrap_or(0);
        plain_bound()
            + BLOCKS_PER_SUB_CHUNK as u64 * MAX_SELECTED_MODEL_TEMPLATES as u64 * model_bytes
    }

    /// What `overlays` cell overlays can add to a sub-chunk's mesh: each draws one model.
    #[must_use]
    pub const fn for_overlays(&self, overlays: usize) -> u64 {
        overlays as u64 * self.fallback_model
    }
}

/// Counts packed streams and biome words, including shared allocation counters.
#[must_use]
pub fn mesh_output_byte_len(mesh: &ChunkMesh, biome: &PackedBiomeRecord) -> u64 {
    bytes::<ChunkMesh>(1)
        + bytes::<CubeStreams>(1)
        + bytes::<ModelDrawRefs>(1)
        + shared_stream_counters()
        + bytes::<PackedQuad>(mesh.cube_quads().len() as u64)
        + bytes::<PackedQuadLighting>(mesh.cube_lighting().len() as u64)
        + bytes::<PackedModelRef>(mesh.model_refs().len() as u64)
        + bytes::<PackedQuadLighting>(mesh.model_lighting().len() as u64)
        + bytes::<PackedModelDrawRef>(
            (mesh.model_draw_refs().len() + mesh.transparent_model_draw_refs().len()) as u64,
        )
        + bytes::<PackedLiquidQuad>(mesh.liquid_quads().len() as u64)
        + bytes::<PackedQuadLighting>(mesh.liquid_lighting().len() as u64)
        + bytes::<DiagnosticGeometryCount>(mesh.diagnostic_geometry().entries().len() as u64)
        + bytes::<crate::PackedTerrainMarker>(mesh.terrain_markers().len() as u64)
        + biome.byte_len()
}

/// Charges one model reference, optional displacement and every quad's packed output.
fn model_part_bound(quads: u64, offset_prefix: bool) -> u64 {
    bytes::<PackedModelRef>(1)
        + if offset_prefix {
            bytes::<PackedQuadLighting>(2)
        } else {
            0
        }
        + quads * (bytes::<PackedQuadLighting>(1) + bytes::<PackedModelDrawRef>(1))
}

/// Reserves all cube and liquid faces, plus the largest packed biome descriptor.
fn plain_bound() -> u64 {
    bytes::<ChunkMesh>(1)
        + bytes::<CubeStreams>(1)
        + bytes::<ModelDrawRefs>(1)
        + shared_stream_counters()
        + bytes::<DiagnosticGeometryCount>(MAX_DIAGNOSTIC_IDENTITIES_PER_MESH as u64)
        + bytes::<u32>(MAX_PACKED_BIOME_RECORD_WORDS as u64)
        + bytes::<crate::PackedTerrainMarker>(BLOCKS_PER_SUB_CHUNK as u64)
        + BLOCKS_PER_SUB_CHUNK as u64
            * Face::ALL.len() as u64
            * (bytes::<PackedQuad>(1)
                + bytes::<PackedLiquidQuad>(1)
                + 2 * bytes::<PackedQuadLighting>(1))
}

/// Reserves strong and weak counters for every shared slice in the publication tuple.
const fn shared_stream_counters() -> u64 {
    // Each slice handle and its two reference counters both occupy two pointer-sized words.
    bytes::<crate::ChunkMeshStreams>(1) + bytes::<std::sync::Arc<[crate::PackedTerrainMarker]>>(1)
}

/// Converts a stream length to bytes without depending on wire layout literals.
const fn bytes<T>(count: u64) -> u64 {
    count * std::mem::size_of::<T>() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_payload_is_charged_even_without_geometry() {
        let mut mesh = ChunkMesh::default();
        let biome = PackedBiomeRecord::fallback();
        let empty_bytes = mesh_output_byte_len(&mesh, &biome);
        mesh.cube_streams.diagnostic_geometry = crate::DiagnosticGeometrySummary::from_counts(
            (0..MAX_DIAGNOSTIC_IDENTITIES_PER_MESH)
                .map(|id| DiagnosticGeometryCount::new(None, id as u32, 1)),
        );
        assert_eq!(
            mesh_output_byte_len(&mesh, &biome) - empty_bytes,
            bytes::<DiagnosticGeometryCount>(MAX_DIAGNOSTIC_IDENTITIES_PER_MESH as u64)
        );
        assert!(mesh_output_byte_len(&mesh, &biome) <= plain_bound());
    }
}
