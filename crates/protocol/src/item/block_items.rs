//! The items server-defined blocks imply.

use std::borrow::Cow;
use std::collections::HashSet;

use bedrock_protocol_1_26_60::enums::ItemVersion;
use bedrock_protocol_1_26_60::packets::{ItemRegistry, StartGame};
use bedrock_protocol_1_26_60::types::ItemData;
use bedrock_protocol_1_26_60::wire;

use super::{MAX_ITEM_REGISTRY_ENTRIES, registry_properties::CANONICAL_EMPTY_COMPONENT_DATA};
use crate::nbt_tree::{Nbt, read_root};

/// Block IDs below this are also their item's ID; higher ones map to `255 - id`.
const FIRST_DERIVED_BLOCK_ID: i64 = 256;

/// Adds the item each server-defined block implies when the registry leaves it out, as an entry
/// matching what servers that do list one send: no components and no item version.
///
/// A custom block's item takes its ID from the `block_id` StartGame declares, so stacks may name
/// an ID the item registry never lists. A registry entry for the same ID or name wins, and server
/// entries keep their place ahead of derived ones when the registry limit is reached.
pub(crate) fn with_block_items<'a>(
    registry: &'a ItemRegistry,
    start_game: &StartGame,
) -> Cow<'a, ItemRegistry> {
    let mut taken_ids = HashSet::with_capacity(registry.item_data.len());
    let mut taken_names = HashSet::with_capacity(registry.item_data.len());
    for item in &registry.item_data {
        taken_ids.insert(item.item_id.0);
        taken_names.insert(item.item_name.to_string_lossy());
    }
    let mut derived = Vec::new();
    let room = MAX_ITEM_REGISTRY_ENTRIES.saturating_sub(registry.item_data.len());
    for property in &start_game.block_properties {
        if derived.len() >= room {
            break;
        }
        let name = property.block_name.to_string_lossy();
        if name.is_empty() || taken_names.contains(&name) {
            continue;
        }
        let Some(network_id) = block_item_network_id(&property.block_definition.0) else {
            continue;
        };
        if !taken_ids.insert(network_id) {
            continue;
        }
        derived.push(ItemData {
            item_name: property.block_name.clone(),
            item_id: wire::I16LE(network_id),
            is_component_based: false,
            item_version: ItemVersion::None,
            item_component_data: wire::NetworkNbt(CANONICAL_EMPTY_COMPONENT_DATA.to_vec().into()),
        });
    }
    if derived.is_empty() {
        return Cow::Borrowed(registry);
    }
    let mut extended = registry.clone();
    extended.item_data.extend(derived);
    Cow::Owned(extended)
}

/// The item network ID of a definition's `vanilla_block_data.block_id`, if it declares a valid one.
fn block_item_network_id(definition: &[u8]) -> Option<i16> {
    let root = read_root(definition)?;
    let Some(Nbt::Int(block_id)) = root.field("vanilla_block_data")?.field("block_id") else {
        return None;
    };
    let block_id = *block_id;
    if block_id < 0 {
        return None;
    }
    let item_id = if block_id < FIRST_DERIVED_BLOCK_ID {
        block_id
    } else {
        255 - block_id
    };
    i16::try_from(item_id).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GameData;
    use bedrock_protocol_1_26_60::types::ServerBlockProperty;

    /// Network NBT for a definition that holds only `vanilla_block_data.block_id`.
    fn definition(block_id: i32) -> Vec<u8> {
        let mut bytes = vec![10, 0, 10, 18];
        bytes.extend_from_slice(b"vanilla_block_data");
        bytes.extend_from_slice(&[3, 8]);
        bytes.extend_from_slice(b"block_id");
        let mut zigzag = ((block_id << 1) ^ (block_id >> 31)) as u32;
        loop {
            let byte = (zigzag & 0x7f) as u8;
            zigzag >>= 7;
            bytes.push(if zigzag == 0 { byte } else { byte | 0x80 });
            if zigzag == 0 {
                break;
            }
        }
        bytes.extend_from_slice(&[0, 0]);
        bytes
    }

    fn start_game(blocks: &[(&str, i32)]) -> StartGame {
        StartGame {
            block_properties: blocks
                .iter()
                .map(|(name, block_id)| ServerBlockProperty {
                    block_name: (*name).into(),
                    block_definition: wire::NetworkNbt(definition(*block_id).into()),
                })
                .collect(),
            ..Default::default()
        }
    }

    fn item(name: &str, id: i16) -> ItemData {
        ItemData {
            item_name: name.into(),
            item_id: wire::I16LE(id),
            ..Default::default()
        }
    }

    fn named_ids(game_data: &GameData) -> Vec<(String, i32)> {
        let registry = game_data
            .items
            .registry
            .as_ref()
            .expect("admitted registry");
        registry
            .entries
            .iter()
            .map(|entry| (entry.identifier.to_string(), entry.network_id))
            .collect()
    }

    #[test]
    fn a_custom_block_the_registry_omits_keeps_the_item_stacks_name_it_by() {
        let game_data = GameData::from_wire(
            &start_game(&[("test:plain", 10114), ("test:other", 10056)]),
            &ItemRegistry {
                item_data: vec![item("minecraft:apple", 878)],
            },
        );
        assert_eq!(
            named_ids(&game_data),
            [
                ("minecraft:apple".to_owned(), 878),
                ("test:plain".to_owned(), -9859),
                ("test:other".to_owned(), -9801),
            ]
        );
        assert!(
            game_data
                .items
                .identifiers
                .iter()
                .any(|identifier| &**identifier == "test:plain")
        );
    }

    #[test]
    fn a_registry_entry_wins_over_the_derived_item() {
        let game_data = GameData::from_wire(
            &start_game(&[("test:listed", 10000), ("test:collides", 10001)]),
            &ItemRegistry {
                item_data: vec![item("test:listed", -9745), item("test:holder", -9746)],
            },
        );
        assert_eq!(
            named_ids(&game_data),
            [
                ("test:listed".to_owned(), -9745),
                ("test:holder".to_owned(), -9746),
            ]
        );
    }

    #[test]
    fn low_block_ids_are_their_own_item_id_and_invalid_ones_add_nothing() {
        let packet = start_game(&[
            ("test:low", 200),
            ("test:negative", -1),
            ("test:huge", 70_000),
        ]);
        let registry = ItemRegistry::default();
        let extended = with_block_items(&registry, &packet);
        let ids = extended
            .item_data
            .iter()
            .map(|item| {
                (
                    item.item_name.to_string_lossy().into_owned(),
                    item.item_id.0,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, [("test:low".to_owned(), 200)]);
    }

    #[test]
    fn a_full_registry_keeps_every_server_entry() {
        let item_data = (0..MAX_ITEM_REGISTRY_ENTRIES)
            .map(|index| item(&format!("test:item{index}"), index as i16))
            .collect();
        let registry = ItemRegistry { item_data };
        let extended = with_block_items(&registry, &start_game(&[("test:block", 20000)]));
        assert!(matches!(extended, Cow::Borrowed(_)));
    }

    #[test]
    fn a_definition_without_a_block_id_adds_nothing() {
        let registry = ItemRegistry::default();
        let mut packet = start_game(&[("test:block", 10000)]);
        packet.block_properties[0].block_definition = wire::NetworkNbt(vec![10, 0, 0].into());
        assert!(matches!(
            with_block_items(&registry, &packet),
            Cow::Borrowed(_)
        ));
    }
}
