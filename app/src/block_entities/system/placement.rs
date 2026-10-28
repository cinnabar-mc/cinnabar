//! Builds fixed item placements with compatible frame visuals.

use std::sync::Arc;

use super::{HeldItem, StaticItemPlacement};

/// Retains a frame's compiled block identity alongside its artwork and fixed pose.
pub(super) fn held_placement(
    item: &HeldItem,
    world_from_item: [[f32; 4]; 3],
    light: Option<(u8, u8)>,
    frame_stream: Option<&chunk_pipeline::WorldStream>,
) -> StaticItemPlacement {
    StaticItemPlacement {
        identifier: Arc::clone(&item.identifier),
        metadata: item.metadata,
        shield_pattern: item.shield_pattern.clone(),
        potted_block: None,
        allow_block_models: frame_stream.is_some(),
        carried_block: frame_stream.and_then(|stream| {
            match stream
                .authority()
                .named_item_visual(&item.identifier, item.metadata)
            {
                assets::ItemVisualRoute::BlockItem(visual) => Some(visual),
                _ => None,
            }
        }),
        world_from_item,
        light,
    }
}

/// The placements of an overlay host's item overlays at `block`: each item flat on its face as
/// an inventory slot shows it, at full brightness. An item on a face past east is left out.
pub(super) fn overlay_item_placements<'a>(
    items: &'a [(u8, world::OverlayItem)],
    block: [i32; 3],
    stream: &'a chunk_pipeline::WorldStream,
) -> impl Iterator<Item = StaticItemPlacement> + 'a {
    items.iter().filter_map(move |(face, item)| {
        let stack = HeldItem {
            shield_pattern: None,
            identifier: Arc::clone(&item.name),
            metadata: u32::from(item.damage.max(0).unsigned_abs()),
        };
        let mut placement = held_placement(&stack, [[0.0; 4]; 3], Some((15, 15)), Some(stream));
        placement.world_from_item = render_model::overlay_item_transform(
            block,
            *face,
            item.size,
            item.x,
            item.y,
            placement.carried_block.is_some(),
        )?;
        Some(placement)
    })
}
