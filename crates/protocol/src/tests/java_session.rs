//! The Go core joins Java Edition 1.7.10 servers and translates them to Bedrock packets.
//!
//! `java_session.bin` is its translation of a scripted join and `java_item_registry.bin` the
//! item registry it announces; `core/javaedition/translate` regenerates and checks both. The
//! client must decode every packet of that stream and accept its startup.

use bedrock_protocol_1_26_60::packets::{self, PacketId};
use bytes::Bytes;
use protocol::decode_batch;

use crate::login::GameData;
use crate::{ActorEvent, PlayerListEntry, PlayerSkin, is_client_default_skin};

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
            PacketId::PlayerSkin,
            PacketId::PlayerList,
            PacketId::AddPlayer,
            PacketId::PlayerSkin,
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
    // Java entity 7, moved up by one past the id Bedrock reserves.
    assert_eq!(start_game.runtime_id.0, 8);
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

/// The core places the player with its own copy of Bedrock's network offset. The scripted
/// server stands the player with its feet on Y 1, so a copy that drifts from the client's
/// offset shows here.
#[test]
fn translated_player_positions_carry_the_network_offset() {
    const SCRIPTED_FEET: f32 = 1.0;
    let session = decode(SESSION);
    let placed = session
        .iter()
        .find_map(|packet| match &*packet.data {
            packets::Packet::MovePlayer(placed) => Some(placed.position.y),
            _ => None,
        })
        .expect("the stream places the player");
    assert_eq!(placed, SCRIPTED_FEET + protocol::PLAYER_NETWORK_OFFSET);
}

/// A Java player is listed without skin pixels, so the client draws its own default skin,
/// then gets the signed skin and cape the core downloaded as a 64x32 classic skin. A player
/// with only a cape gets it without skin pixels, to be drawn over the client's default skin.
#[test]
fn translated_java_skins_reach_the_client() {
    let session = decode(SESSION);
    let listed = session
        .iter()
        .find_map(|packet| match &*packet.data {
            packets::Packet::PlayerList(list) => Some(list.clone()),
            _ => None,
        })
        .expect("the stream lists the remote player");
    let ActorEvent::PlayerList(list) =
        crate::actor::normalize_player_list(listed).expect("the entry decodes")
    else {
        panic!("a player list decodes to a player-list event");
    };
    let [PlayerListEntry::Add { skin, .. }] = &*list.entries else {
        panic!("one added entry");
    };
    assert!(
        is_client_default_skin(skin),
        "the listed skin asks for the client's default, got {skin:?}"
    );

    let mut updates = session.iter().filter_map(|packet| match &*packet.data {
        packets::Packet::PlayerSkin(update) => {
            let ActorEvent::Skin { skin, .. } = crate::actor::normalize_skin_update(update.clone())
            else {
                panic!("a skin update decodes to a skin event");
            };
            Some(skin)
        }
        _ => None,
    });
    let PlayerSkin::Standard(downloaded) = updates.next().expect("the downloaded skin") else {
        panic!("the downloaded skin is usable");
    };
    // The 64x32 skin is expanded to the 64x64 layout with mirrored limbs.
    assert_eq!((downloaded.width, downloaded.height), (64, 64));
    let cape = downloaded.cape.expect("the cape is kept");
    assert_eq!((cape.width, cape.height), (64, 32));

    let cape_only = updates.next().expect("the cape over the default skin");
    assert!(is_client_default_skin(&cape_only), "got {cape_only:?}");
    let PlayerSkin::Standard(cape_only) = cape_only else {
        unreachable!()
    };
    let cape = cape_only
        .cape
        .expect("the cape is kept without skin pixels");
    assert_eq!((cape.width, cape.height), (64, 32));
}
