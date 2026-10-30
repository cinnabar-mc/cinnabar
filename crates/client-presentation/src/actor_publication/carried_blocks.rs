//! Blocks actor renderers draw with their bodies, published with the world's block models.

use std::collections::HashMap;

use chunk_pipeline::WorldStream;
use protocol::{ActorKind, ActorMetadataValue};
use protocol_ids::ActorDataKey;

use crate::presentation::actors::ActorPresentationBatch;

#[cfg(test)]
mod tests;

/// A block an actor carries, placed for this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CarriedBlock {
    /// The block's wire runtime id, which may name air.
    pub(crate) block_runtime_id: u32,
    /// Maps the centred unit block into the world.
    pub(crate) world_from_block: [[f32; 4]; 3],
    /// The carrier body's `(block, sky)` light.
    pub(crate) light: (u8, u8),
}

/// The light a carried block draws with while its carrier's light is unsolved. The body then
/// draws white without the lightmap, so the block takes the lightmap's brightest sample instead
/// of solved darkness.
const UNSOLVED_CARRIER_LIGHT: (u8, u8) = (15, 15);

/// The `(block, sky)` light a carried block draws with, given its carrier body's light.
fn carried_light(body_light: Option<(u8, u8)>) -> (u8, u8) {
    body_light.unwrap_or(UNSOLVED_CARRIER_LIGHT)
}

/// The wire block runtime id an actor's native renderer draws as carried: only endermen draw
/// their synced carried block. Air is returned as synced; the caller skips it.
fn carried_block_runtime_id(
    kind: &ActorKind,
    metadata: &HashMap<u32, ActorMetadataValue>,
) -> Option<u32> {
    let ActorKind::Entity { identifier } = kind else {
        return None;
    };
    if identifier.as_ref() != assets::facts::entity::ENDERMAN_IDENTIFIER {
        return None;
    }
    match metadata.get(&ActorDataKey::CarryBlockRuntimeID.wire_value())? {
        ActorMetadataValue::Int(id) => Some(u32::from_ne_bytes(id.to_ne_bytes())),
        _ => None,
    }
}

/// Places the carried block of every drawn body in `batch`, lit like its carrier.
pub(super) fn carried_blocks_for(
    batch: &ActorPresentationBatch,
    stream: &WorldStream,
) -> Vec<CarriedBlock> {
    batch
        .submissions
        .iter()
        .filter(|body| body.input.identity.layer == render_model::ACTOR_LAYER_BODY)
        .filter_map(|body| {
            let actor = stream.authority().actor(body.input.identity.runtime_id)?;
            let block_runtime_id = carried_block_runtime_id(&actor.kind, &actor.metadata)?;
            let world_from_block = render_model::dropped_item::enderman_carried_block_transform(
                body.world_from_actor,
                actor.render_scale(),
            )?;
            let light = carried_light(crate::presentation::actors::body_light(body, stream));
            Some(CarriedBlock {
                block_runtime_id,
                world_from_block,
                light,
            })
        })
        .collect()
}
