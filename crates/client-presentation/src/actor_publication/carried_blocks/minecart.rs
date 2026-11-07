//! Minecart display metadata and constructor-owned default contents.

use super::{CarriedBlockId, HashMap, OnceLock};
use assets::facts::entity::{EntityFacts, NativeEntityClass};
use client_world::ActorSnapshot;
use protocol::ActorMetadataValue;
use protocol_ids::ActorDataKey;

/// Resolves contents only for admitted minecart constructors, including advertised native bases.
pub(super) fn display(actor: &ActorSnapshot) -> Option<(CarriedBlockId, i32)> {
    if actor.entity_fact.native_class != NativeEntityClass::Minecart {
        return None;
    }
    let custom = matches!(actor.metadata.get(&ActorDataKey::CustomDisplay.wire_value()),
        Some(ActorMetadataValue::Byte(value)) if *value != 0);
    let block = if custom {
        match actor
            .metadata
            .get(&ActorDataKey::DisplayBlockState.wire_value())?
        {
            ActorMetadataValue::Int(id) => {
                CarriedBlockId::Wire(u32::from_ne_bytes(id.to_ne_bytes()))
            }
            _ => return None,
        }
    } else {
        CarriedBlockId::Pinned(*default_blocks().get(&actor.entity_fact.class_key)?)
    };
    let offset = match actor
        .metadata
        .get(&ActorDataKey::DisplayOffset.wire_value())
    {
        Some(ActorMetadataValue::Int(offset)) => *offset,
        _ => render_model::dropped_item::MINECART_DEFAULT_DISPLAY_OFFSET,
    };
    Some((block, offset))
}

/// Derives each built-in variant's block from its pinned constructor, never a server's suffix.
fn default_blocks() -> &'static HashMap<u16, u32> {
    static BLOCKS: OnceLock<HashMap<u16, u32>> = OnceLock::new();
    BLOCKS.get_or_init(|| {
        EntityFacts::pinned()
            .records
            .iter()
            .filter_map(|record| {
                if record.fact.native_class != NativeEntityClass::Minecart {
                    return None;
                }
                let name = record.identifier.strip_suffix("_minecart")?;
                let block = assets::pinned_block_state_by_name(name)?;
                Some((record.fact.class_key, block))
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pinned_cart_variant_resolves_its_constructor_block() {
        for record in EntityFacts::pinned()
            .records
            .iter()
            .filter(|record| record.fact.native_class == NativeEntityClass::Minecart)
        {
            let Some(name) = record.identifier.strip_suffix("_minecart") else {
                assert_eq!(
                    record.identifier.as_ref(),
                    "minecraft:minecart",
                    "new bare cart needs qualification"
                );
                assert!(!default_blocks().contains_key(&record.fact.class_key));
                continue;
            };
            let id = default_blocks()
                .get(&record.fact.class_key)
                .expect("new cart variant needs a display block");
            assert_eq!(assets::pinned_block_identity(*id).unwrap().0, name);
        }
    }
}
