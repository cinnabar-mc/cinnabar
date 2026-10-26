//! Carried walls retain their posts and separate rail halves.

use super::*;

/// Builds the carried wall independently of placed connections and inventory orientation.
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
    template_quads(&assets::gui_item::carried_wall_quads(
        block.face(BlockFace::South).material_id(),
    ))
}
