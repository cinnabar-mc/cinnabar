//! Ordinary carried gates retain artwork directions independently of world orientation.

use super::*;

/// Reuses the admitted gate geometry with carried post caps and rail artwork.
pub(super) fn model(
    world: &RuntimeAssets,
    visual: u32,
) -> Option<([u32; 6], Vec<crate::ActorRigVertex>)> {
    let mut quads = carried_quads(world, visual)?;
    let (bottom, top) = quads
        .iter()
        .flat_map(|q| q.corners)
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p[1]), hi.max(p[1]))
        });
    for quad in &mut quads {
        let [a, b, c, _] = quad.corners.map(glam::Vec3::from);
        let normal = (b - a).cross(c - a);
        let (zmin, zmax) = quad
            .corners
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p[2]), hi.max(p[2]))
            });
        let near_post_cap = normal.y != 0.0 && zmax < 0.5 && (a.y == bottom || a.y == top);
        quad.uvs = quad.corners.map(|[x, y, z]| {
            if normal.y > 0.0 {
                if near_post_cap {
                    [x, z - zmin]
                } else {
                    [1.0 - z, x]
                }
            } else if normal.y < 0.0 {
                if near_post_cap {
                    [x, zmax - z]
                } else {
                    [zmin + zmax - z, 1.0 - x]
                }
            } else if normal.z < 0.0 {
                [1.0 - x, 1.0 - y]
            } else if normal.z > 0.0 {
                [x, 1.0 - y]
            } else if normal.x < 0.0 {
                [z, 1.0 - y]
            } else {
                [zmin + zmax - z, 1.0 - y]
            }
        });
    }
    template_quads(&quads)
}

/// Retains active carried gate geometry and material regions for artwork placement.
pub(super) fn carried_quads(
    world: &RuntimeAssets,
    visual: u32,
) -> Option<Vec<assets::gui_item::GuiBlockQuad>> {
    let block = world.resolve(NetworkIdMode::Sequential, visual);
    if block.kind() != VisualKind::Model
        || block.support() != VisualSupport::Exact
        || block.animation().is_some()
    {
        return None;
    }
    let template = block.model_template()?;
    let flags = world.model_templates().get(template as usize)?.flags;
    if flags & (assets::MODEL_TEMPLATE_FLAG_GATE_AXIS_X | assets::MODEL_TEMPLATE_FLAG_GATE_AXIS_Z)
        == 0
    {
        return None;
    }
    assets::gui_item::block_item_quads(
        block.kind(),
        BlockFace::ALL.map(|face| block.face(face).material_id()),
        Some(template),
        world.model_templates(),
        world.model_quads(),
    )
    .ok()
}
