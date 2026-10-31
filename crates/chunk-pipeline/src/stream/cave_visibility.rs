use super::*;

impl WorldStream {
    /// Extends retained visibility or polls a full search without waiting for it.
    /// Show all sections on `Unavailable`; swap `replacement` into `visible` on `Rebuilt`.
    /// Keep the worker, scratch and both output buffers together between updates.
    pub fn update_cave_visible_sub_chunks(
        &self,
        camera: SubChunkKey,
        rebuild: &mut crate::CaveVisibilityRebuild,
        scratch: &mut crate::CaveVisibilityScratch,
        visible: &mut crate::CaveVisibleSet,
        replacement: &mut crate::CaveVisibleSet,
    ) -> crate::CaveVisibilityUpdate {
        rebuild.update(camera, &self.connectivity, scratch, visible, replacement)
    }

    /// Reuses the caller's visibility buffers for the current connectivity graph.
    pub fn cave_visible_sub_chunks_into(
        &self,
        camera: SubChunkKey,
        scratch: &mut crate::CaveVisibilityScratch,
        visible: &mut crate::CaveVisibleSet,
    ) {
        crate::culling::fill_visible(camera, &self.connectivity, scratch, visible);
    }
}
