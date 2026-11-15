use bedrock_protocol_1_26_60::packets::PlayerSkin as PlayerSkinPacket;

use super::{ActorEvent, EmptySkin, normalize_player_skin};

/// Retains an appearance change without inventing a player-list entry; `empty` decides how a
/// skin without pixels decodes.
pub(crate) fn normalize_skin_update(packet: PlayerSkinPacket, empty: EmptySkin) -> ActorEvent {
    ActorEvent::Skin {
        uuid: *packet.uuid.as_bytes(),
        skin: normalize_player_skin(packet.serialized_skin, &mut 0, empty),
    }
}
