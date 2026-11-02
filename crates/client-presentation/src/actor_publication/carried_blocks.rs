//! Blocks actor renderers draw with their bodies, published with the world's block models.

use std::collections::HashMap;
use std::sync::OnceLock;

use assets::facts::{BlockProperty, PropertyDomain, pack::PackBlock};
use chunk_pipeline::WorldStream;
use protocol::{ActorKind, ActorMetadataValue};
use protocol_ids::ActorDataKey;

use crate::presentation::actors::ActorPresentationBatch;

#[cfg(test)]
mod tests;

/// Which block an actor's renderer draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CarriedBlockId {
    /// A wire runtime id the server synced, which may name air.
    Wire(u32),
    /// A state of the pinned registry the renderer chose itself.
    Pinned(u32),
}

/// A block an actor carries, placed for this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CarriedBlock {
    pub(crate) block: CarriedBlockId,
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

/// The carved pumpkin state a snow golem wears: the block's default, the first value of its
/// direction domain, whose carved face a golem's rest pose looks forward through.
fn snow_golem_pumpkin() -> Option<u32> {
    static STATE: OnceLock<Option<u32>> = OnceLock::new();
    *STATE.get_or_init(|| {
        let name = PackBlock::CarvedPumpkin.identifier();
        let first = assets::pinned_block_state_by_name(name)?;
        let property = BlockProperty::MinecraftCardinalDirection;
        let PropertyDomain::String(directions) = property.domain() else {
            return None;
        };
        (first..first.saturating_add(directions.len() as u32)).find(|&state| {
            assets::pinned_block_identity(state).is_some_and(|(state_name, canonical)| {
                state_name == name && canonical.contains(directions[0])
            })
        })
    })
}

/// Whether the native renderer draws a pumpkin on this actor: every snow golem not yet sheared.
fn wears_pumpkin(kind: &ActorKind, sheared: bool) -> bool {
    matches!(kind, ActorKind::Entity { identifier }
        if identifier.as_ref() == assets::facts::entity::SNOW_GOLEM_IDENTIFIER)
        && !sheared
}

/// Places the block every drawn body in `batch` carries or wears, lit like its carrier.
/// `frame` is the render frame that posed the bodies and `partial_tick` its fraction, so a worn
/// block turns with the head its body draws. A golem whose rig has no head part wears nothing,
/// like the native layer.
pub(super) fn carried_blocks_for(
    batch: &ActorPresentationBatch,
    stream: &WorldStream,
    frame: &mut actor_animation::ActorRenderFrame<'_>,
    partial_tick: f32,
) -> Vec<CarriedBlock> {
    batch
        .submissions
        .iter()
        .filter(|body| body.input.identity.layer == render_model::ACTOR_LAYER_BODY)
        .filter_map(|body| {
            let actor = stream.authority().actor(body.input.identity.runtime_id)?;
            let (block, world_from_block) = if wears_pumpkin(&actor.kind, actor.is_sheared()) {
                let head = frame.head_rotation(actor.runtime_id)?;
                (
                    CarriedBlockId::Pinned(snow_golem_pumpkin()?),
                    render_model::dropped_item::snow_golem_pumpkin_transform(
                        body.world_from_actor,
                        head.previous,
                        head.current,
                        partial_tick,
                    )?,
                )
            } else {
                (
                    CarriedBlockId::Wire(carried_block_runtime_id(&actor.kind, &actor.metadata)?),
                    render_model::dropped_item::enderman_carried_block_transform(
                        body.world_from_actor,
                        actor.render_scale(),
                    )?,
                )
            };
            let light = carried_light(crate::presentation::actors::body_light(body, stream));
            Some(CarriedBlock {
                block,
                world_from_block,
                light,
            })
        })
        .collect()
}
