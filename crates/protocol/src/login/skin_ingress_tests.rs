//! A skin without pixels decodes by the session's rule through encoded world ingress.
use super::*;
use crate::raw_packet::decode_packet_raw;
use crate::{ActorEvent, EmptySkin, PlayerListEntry, PlayerSkin, PlayerSkinUnavailable};
use bedrock_protocol_1_26_60::packets::PlayerList as PlayerListPacket;
use bedrock_protocol_1_26_60::types::{
    ActorUniqueID, PlayerListData, SerializedSkinRef, SkinImage,
};
use bedrock_protocol_1_26_60::wire;
use bytes::Buf;

/// A player list adding one player whose skin has no pixels but a 64x32 cape.
fn empty_skin_with_cape() -> Packet {
    Packet::new(PlayerListPacket {
        entries: vec![PlayerListData::AddEntry {
            play_fab_id: Default::default(),
            action: Default::default(),
            uuid: Default::default(),
            actor_unique_id: ActorUniqueID(9),
            player_name: "Alex".into(),
            xbl_xuid: Default::default(),
            platform_online_id: Default::default(),
            build_platform: Default::default(),
            serialized_skin: Box::new(SerializedSkinRef {
                cape_image_data: SkinImage {
                    width: wire::U32LE(64),
                    height: wire::U32LE(32),
                    image_bytes: vec![7; 64 * 32 * 4].into(),
                },
                ..Default::default()
            }),
            is_teacher: false,
            is_host: false,
            is_sub_client: false,
            player_color: Default::default(),
        }],
    })
}

/// The listed skin after encoded ingress under `empty`.
fn listed_skin(empty: EmptySkin) -> PlayerSkin {
    let mut batch = crate::encode(&empty_skin_with_cape()).unwrap();
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).unwrap();
    let Some(WorldEvent::Actor(ActorEvent::PlayerList(list))) =
        decode_world_raw_for(raw, 0, empty, super::decode_test_raw).unwrap()
    else {
        panic!("a player list decodes to a player-list event");
    };
    let [PlayerListEntry::Add { skin, .. }] = &*list.entries else {
        panic!("one added entry");
    };
    skin.clone()
}

/// Bedrock sessions keep an empty skin unusable, as before; Java Edition sessions keep its
/// cape over the client's default skin.
#[test]
fn an_empty_skin_follows_the_sessions_rule() {
    assert_eq!(
        listed_skin(EmptySkin::Unavailable),
        PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions)
    );
    let java = listed_skin(EmptySkin::ClientDefault);
    assert!(crate::is_client_default_skin(&java), "got {java:?}");
    let PlayerSkin::Standard(java) = java else {
        unreachable!()
    };
    assert_eq!(
        java.cape.map(|cape| (cape.width, cape.height)),
        Some((64, 32))
    );
}
