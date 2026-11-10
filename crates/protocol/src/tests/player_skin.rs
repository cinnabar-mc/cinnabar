use bedrock_protocol_1_26_60::packets::PlayerSkin as PlayerSkinPacket;
use bedrock_protocol_1_26_60::types::{SerializedSkin, SerializedSkinImage};
use bedrock_protocol_1_26_60::wire;
use protocol::{ActorEvent, PlayerSkin, WorldEvent, into_world_event};
use render_api::CLASSIC_SKIN_SIDE;

#[test]
fn player_skin_packet_reaches_the_actor_pipeline() {
    let packet = PlayerSkinPacket {
        uuid: uuid::Uuid::from_bytes([7; 16]),
        serialized_skin: SerializedSkin {
            image_data: SerializedSkinImage {
                width: wire::U32LE(CLASSIC_SKIN_SIDE as u32),
                height: wire::U32LE(CLASSIC_SKIN_SIDE as u32),
                image_bytes: vec![255; CLASSIC_SKIN_SIDE * CLASSIC_SKIN_SIDE * 4].into(),
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let Some(WorldEvent::Actor(ActorEvent::Skin { uuid, skin })) =
        into_world_event(protocol::Packet::new(packet), 0).unwrap()
    else {
        panic!("a skin update must reach the actor pipeline");
    };
    assert_eq!(uuid, [7; 16]);
    assert!(
        matches!(skin, PlayerSkin::Standard(skin) if skin.rgba8.iter().all(|pixel| *pixel == 255))
    );
}

#[test]
fn player_skin_updates_keep_a_visible_head_and_transparent_body() {
    let side = CLASSIC_SKIN_SIDE;
    let mut pixels = vec![0; side * side * 4];
    for y in 0..16 {
        for x in 0..side {
            let at = (y * side + x) * 4;
            pixels[at..at + 4].copy_from_slice(&[80, 40, 20, 255]);
        }
    }
    let packet = PlayerSkinPacket {
        serialized_skin: SerializedSkin {
            resource_patch: r#"{"geometry":{"default":"geometry.humanoid.custom"}}"#.into(),
            image_data: SerializedSkinImage {
                width: wire::U32LE(side as u32),
                height: wire::U32LE(side as u32),
                image_bytes: pixels.clone().into(),
            },
            overrides_player_appearance: true,
            trusted_skin_flag: "true".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let Some(WorldEvent::Actor(ActorEvent::Skin {
        skin: PlayerSkin::Standard(skin),
        ..
    })) = into_world_event(protocol::Packet::new(packet), 0).unwrap()
    else {
        panic!("skin update retains a supported skin");
    };
    let changed = skin.rgba8.iter().zip(&pixels).position(|(a, b)| a != b);
    assert_eq!(
        changed, None,
        "skin update must not fill the transparent body"
    );
}
