//! Dust the local player's brush sweeps off the block it is used on.

use bevy::prelude::Message;
use particles::{ParticleSystem, brush_dust_request};

use super::{BLOCK_BREAK_EFFECT, Routing, block_tile};

/// One local brush stroke whose use on a block draws dust, as vanilla's client does from
/// `BrushItem::_useOn` once its effects cooldown has passed.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub(crate) struct LocalBrushDust {
    /// The brushed block.
    pub block: [i32; 3],
    /// The face the stroke hit.
    pub face: u8,
    /// The stroke's hit position within the block, from its minimum corner.
    pub relative_hit: [f32; 3],
    /// The brushed block's runtime id when the stroke was sent.
    pub runtime_id: u32,
}

/// Draws a stroke's dust with the brushed block's texture and tint; an untextured block
/// draws nothing.
pub(super) fn spawn_brush_dust(
    system: &mut ParticleSystem,
    routing: &Routing<'_>,
    stroke: &LocalBrushDust,
) {
    let Some(found) = block_tile(
        routing.stream,
        routing.mode,
        stroke.runtime_id,
        stroke.block,
    ) else {
        return;
    };
    let hit = std::array::from_fn(|axis| stroke.block[axis] as f32 + stroke.relative_hit[axis]);
    system.spawn_terrain(&brush_dust_request(
        BLOCK_BREAK_EFFECT,
        stroke.block,
        stroke.face,
        hit,
        found.tile,
        found.tint,
    ));
}
