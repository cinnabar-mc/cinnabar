//! Carried fences retain their ordinary or bamboo geometry and texture layout.

use super::*;

/// Builds the carried fence from its admitted inventory-side material.
pub(super) fn model(
    world: &RuntimeAssets,
    visual: u32,
    bamboo: bool,
) -> Option<([u32; 6], Vec<crate::ActorRigVertex>)> {
    let block = world.resolve(NetworkIdMode::Sequential, visual);
    if block.kind() != VisualKind::Model
        || block.support() != VisualSupport::Exact
        || block.model_template().is_none()
        || block.animation().is_some()
    {
        return None;
    }
    let material = block.face(BlockFace::South).material_id();
    let quads = if bamboo {
        assets::gui_item::carried_bamboo_fence_quads(material)
    } else {
        assets::gui_item::carried_fence_quads(material)
    };
    template_quads(&quads)
}
