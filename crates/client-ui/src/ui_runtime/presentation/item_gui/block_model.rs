//! Block items with overlapping faces as GUI geometry: the quads their baked thumbnail
//! projects, painted in their supplied order at the control's physical size.

use std::{collections::BTreeMap, sync::Arc};

use assets::gui_item::{
    CubeLighting, GUI_BLOCK_SAMPLE_ALPHA_THRESHOLD as ALPHA_CUTOFF, GuiBlockQuad, face_brightness,
    project_cube,
};
use ui::{UiMesh, UiMeshBatch};

use {
    super::{BlockModelOrder, atlas_uv, batch, shade, vertex},
    assets::gui_item::GUI_ITEM_SIDE,
    ui::IconRef,
};

/// Each quad with its tile's place in the atlas. `None` when a UV leaves its tile, which an atlas
/// cannot wrap.
pub(in crate::ui_runtime::presentation) fn mesh(
    quads: &[(GuiBlockQuad, IconRef)],
    lighting: CubeLighting,
    order: BlockModelOrder,
) -> Option<Arc<UiMesh>> {
    let mut vertices = Vec::with_capacity(quads.len() * 4);
    let mut indices = Vec::with_capacity(quads.len() * 6);
    let mut batches: Vec<UiMeshBatch> = Vec::new();
    let mut by_page: BTreeMap<u16, Vec<u32>> = BTreeMap::new();
    for (quad, texture) in quads {
        if !quad.visible_in_gui() {
            continue;
        }
        if quad
            .uvs
            .iter()
            .flatten()
            .any(|value| !(0.0..=1.0).contains(value))
        {
            return None;
        }
        let shade = shade(face_brightness(quad.corners), lighting);
        let start = vertices.len() as u32;
        for (corner, uv) in quad.corners.into_iter().zip(quad.uvs) {
            let [x, y, z] = project_cube(corner);
            let mut vertex = vertex(
                [x / GUI_ITEM_SIDE, y / GUI_ITEM_SIDE],
                atlas_uv(*texture, uv)?,
                [shade, shade, shade, 255],
                texture.glint,
                true,
            );
            if order == BlockModelOrder::Depth {
                vertex.clip_z = 0.5 - z / 8.0;
            }
            vertex.terrain_mips = true;
            vertices.push(vertex);
        }
        let quad_indices = [0, 1, 2, 0, 2, 3].map(|index| start + index);
        if order == BlockModelOrder::Depth {
            by_page
                .entry(texture.page)
                .or_default()
                .extend(quad_indices);
            continue;
        }
        let begin = indices.len() as u32;
        indices.extend(quad_indices);
        let end = indices.len() as u32;
        if let Some(previous) = batches.last_mut()
            && previous.texture_page == texture.page
        {
            previous.index_range.end = end;
        } else {
            batches.push(batch(texture.page, begin..end, Some(ALPHA_CUTOFF)));
        }
    }
    for (page, page_indices) in by_page {
        let begin = indices.len() as u32;
        indices.extend(page_indices);
        let mut batch = batch(page, begin..indices.len() as u32, Some(ALPHA_CUTOFF));
        batch.depth_test = true;
        batch.depth_write = true;
        batches.push(batch);
    }
    UiMesh::new(vertices.into(), indices.into(), batches.into())
        .ok()
        .map(UiMesh::with_compact_block_vertices)
        .map(Arc::new)
}

#[cfg(test)]
mod tests;
