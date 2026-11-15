//! Normalization of raw world-lane packets into world events.
use super::*;

/// [`decode_world_raw_for`] as a Bedrock session decodes, for tests of packets that carry no
/// skin rule of their own.
#[cfg(test)]
pub(super) fn decode_world_raw_with(
    raw: RawPacket,
    current_dimension: i32,
    decode: impl FnOnce(RawPacket) -> Result<(GameHeader, packets::Packet), SessionError>,
) -> Result<Option<WorldEvent>, ProtocolError> {
    decode_world_raw_for(
        raw,
        current_dimension,
        crate::EmptySkin::Unavailable,
        decode,
    )
}

/// Normalizes one raw world-lane packet; `decode` supplies the generic payload decode for the
/// packets no bounded raw decoder owns, and `empty_skin` is the session's rule for skins
/// without pixels.
pub(super) fn decode_world_raw_for(
    raw: RawPacket,
    current_dimension: i32,
    empty_skin: crate::EmptySkin,
    decode: impl FnOnce(RawPacket) -> Result<(GameHeader, packets::Packet), SessionError>,
) -> Result<Option<WorldEvent>, ProtocolError> {
    if raw.id == PacketId::ItemRegistry {
        // Login has already initialized this session's registry. Vanilla 1.26.60
        // ignores a repeated item registry once initialized, including an
        // empty/custom-only repeat. Decode the wire first so malformed repeats remain fatal;
        // neither the inventory ledger nor actor item store may be rebound here.
        decode(raw).map_err(crate::codec::world_decode_error)?;
        return Ok(None);
    }
    if raw.id == PacketId::UpdateAbilities {
        return crate::decode_abilities_update(raw.body())
            .map(WorldEvent::Abilities)
            .map(Some);
    }
    if raw.id == PacketId::CraftingData {
        let update = crate::decode_recipe_update(raw.body())
            .map_err(crate::world::WorldPacketError::from)?;
        return Ok(Some(WorldEvent::Inventory(crate::InventoryEvent::Recipes(
            update,
        ))));
    }
    if raw.id == PacketId::UnlockedRecipes {
        let update = crate::decode_recipe_unlocks(raw.body())
            .map_err(crate::world::WorldPacketError::from)?;
        return Ok(Some(WorldEvent::Inventory(
            crate::InventoryEvent::RecipeUnlocks(update),
        )));
    }
    // Play decodes only the packets world normalization consumes; the rest stay undecoded.
    if !crate::world::routes_world_packet(raw.id) {
        return Ok(None);
    }
    if raw.id == PacketId::MoveActorAbsolute {
        return Ok(Some(WorldEvent::Actor(
            crate::actor::normalize_move_entity_body(raw.body(), current_dimension)
                .map_err(crate::world::WorldPacketError::from)?,
        )));
    }
    crate::inventory::validate_raw_inventory_packet(&raw)
        .map_err(crate::world::WorldPacketError::from)?;
    if raw.id == PacketId::MobEquipment
        && let Some(equipment) = raw_equipment::decode_empty_mob_equipment(&raw)?
    {
        return Ok(Some(WorldEvent::Equipment(equipment)));
    }
    if let Some((header, data)) = level_event_generic(&raw) {
        return Ok(crate::world::dispatch(
            data,
            header,
            current_dimension,
            empty_skin,
        )?);
    }
    let (header, data) = decode(raw).map_err(crate::codec::world_decode_error)?;
    crate::codec::validate_decoded(&data).map_err(demote_ui_semantic_rejection)?;
    Ok(crate::world::dispatch(
        data,
        header,
        current_dimension,
        empty_skin,
    )?)
}
