//! Ordinary carried fences keep centered rail splits and post-relative artwork.

use super::*;

/// Builds the carried fence from its admitted inventory-side material.
pub(super) fn model(
    world: &RuntimeAssets,
    visual: u32,
) -> Option<([u32; 6], Vec<crate::ActorRigVertex>)> {
    let block = world.resolve(NetworkIdMode::Sequential, visual);
    if block.kind() != VisualKind::Model
        || block.support() != VisualSupport::Exact
        || block.model_template().is_none()
        || block.animation().is_some()
    {
        return None;
    }
    template_quads(&assets::gui_item::carried_fence_quads(
        block.face(BlockFace::South).material_id(),
    ))
}
