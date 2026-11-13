//! The Go core joins Java Edition 1.7.10 servers and translates them to Bedrock packets.
//!
//! `java_session.bin` is its translation of a scripted join and `java_item_registry.bin` the
//! item registry it announces; `core/javaedition/translate` regenerates and checks both. The
//! client must decode every packet of that stream and accept its startup.

use bedrock_protocol_1_26_60::packets::{self, PacketId};
use bytes::Bytes;
use protocol::decode_batch;

use crate::login::GameData;

const SESSION: &[u8] = include_bytes!("../../fixtures/java_session.bin");
const ITEM_REGISTRY: &[u8] = include_bytes!("../../fixtures/java_item_registry.bin");

fn decode(fixture: &'static [u8]) -> Vec<protocol::Packet> {
    decode_batch(Bytes::from_static(fixture)).expect("the translated packets decode")
}

#[test]
fn translated_java_session_decodes_in_order() {
    let ids: Vec<PacketId> = decode(SESSION)
        .iter()
        .map(|packet| packet.header.id)
        .collect();
    assert_eq!(
        ids,
        [
            PacketId::StartGame,
            PacketId::LevelChunk,
            PacketId::NetworkChunkPublisherUpdate,
            PacketId::MovePlayer,
            PacketId::UpdateAttributes,
            PacketId::UpdateAbilities,
            PacketId::PlayStatus,
            PacketId::SetTime,
            PacketId::Text,
            PacketId::UpdateAttributes,
            PacketId::PlayerList,
            PacketId::AddPlayer,
            PacketId::MoveActorAbsolute,
            PacketId::UpdateBlock,
            PacketId::UpdateBlock,
            PacketId::InventorySlot,
            PacketId::RemoveActor,
        ]
    );
}

#[test]
fn translated_java_startup_builds_game_data() {
    let session = decode(SESSION);
    let packets::Packet::StartGame(start_game) = &*session[0].data else {
        panic!("the stream starts with StartGame");
    };
    assert_eq!(start_game.runtime_id.0, 7);
    assert!(
        start_game.block_network_ids_are_hashes,
        "blocks are addressed by state hash"
    );

    let registry = decode(ITEM_REGISTRY);
    assert_eq!(registry.len(), 1);
    let packets::Packet::ItemRegistry(item_registry) = &*registry[0].data else {
        panic!("the registry fixture holds an ItemRegistry");
    };
    let game_data = GameData::from_wire(start_game, item_registry);
    let admitted = game_data
        .items
        .registry
        .as_ref()
        .expect("the registry is admitted");
    let has = |identifier: &str| {
        admitted
            .entries
            .iter()
            .any(|entry| &*entry.identifier == identifier)
    };
    assert!(has("minecraft:diamond_sword") && has("minecraft:stone") && has("minecraft:apple"));
    assert!(
        admitted.entries.len() > 1500,
        "{} items",
        admitted.entries.len()
    );
}
