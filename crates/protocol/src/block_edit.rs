//! Outbound packets for block-side requests: sign and command block edits and map image requests.

use bedrock_protocol_1_26_60::packets::{BlockActorData, CommandBlockUpdate, MapInfoRequest};
use bedrock_protocol_1_26_60::types::{ActorUniqueID, BlockPos, CommandBlockUpdateData};
use bedrock_protocol_1_26_60::wire;
use bytes::Bytes;

use crate::Packet;

/// The `BlockActorData` a client sends when it closes a sign editor; `nbt` is the whole sign
/// compound as NetworkLittleEndian bytes.
#[must_use]
pub fn sign_edit_packet(position: [i32; 3], nbt: &[u8]) -> Packet {
    crate::Packet::new(BlockActorData {
        block_position: BlockPos {
            x: wire::ZigZag32(position[0]),
            y: wire::ZigZag32(position[1]),
            z: wire::ZigZag32(position[2]),
        },
        actor_data_tags: wire::NetworkNbt(Bytes::copy_from_slice(nbt)),
    })
}

/// What a client reports about one command block when its editor closes.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommandBlockFields {
    pub position: [i32; 3],
    /// 0 impulse, 1 repeat, 2 chain.
    pub mode: u32,
    pub needs_redstone: bool,
    pub conditional: bool,
    pub command: String,
    pub last_output: String,
    pub name: String,
    pub filtered_name: String,
    pub track_output: bool,
    pub tick_delay: i32,
    pub execute_on_first_tick: bool,
}

/// The `CommandBlockUpdate` of a block's command block.
#[must_use]
pub fn command_block_update_packet(fields: &CommandBlockFields) -> Packet {
    crate::Packet::new(CommandBlockUpdate {
        target: CommandBlockUpdateData::BlockCommandData {
            block_position: BlockPos {
                x: wire::ZigZag32(fields.position[0]),
                y: wire::ZigZag32(fields.position[1]),
                z: wire::ZigZag32(fields.position[2]),
            },
            command_block_mode: wire::VarUInt(fields.mode),
            redstone_mode: fields.needs_redstone,
            is_conditional: fields.conditional,
        },
        command: fields.command.as_str().into(),
        last_output: fields.last_output.as_str().into(),
        name: fields.name.as_str().into(),
        filtered_name: fields.filtered_name.as_str().into(),
        track_output: fields.track_output,
        tick_delay: wire::I32LE(fields.tick_delay),
        execute_on_first_tick: fields.execute_on_first_tick,
    })
}

/// Asks the server for the pixels of `map_id`, as a client does on first sight of a map.
#[must_use]
pub fn map_info_request_packet(map_id: i64) -> Packet {
    crate::Packet::new(MapInfoRequest {
        map_unique_id: ActorUniqueID(map_id),
        client_pixels_list: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders_wrap_the_expected_wire_packets() {
        let sign = sign_edit_packet([1, 2, 3], &[10, 0, 0]);
        let map = map_info_request_packet(9);
        assert_ne!(format!("{sign:?}"), format!("{map:?}"));
    }

    /// The fields of a block's update reach the wire in the order the client's payload writes them.
    #[test]
    fn a_command_block_update_writes_its_block_target_and_fields() {
        let packet = command_block_update_packet(&CommandBlockFields {
            position: [2, -60, 0],
            mode: 1,
            needs_redstone: true,
            conditional: false,
            command: "say hi".into(),
            last_output: "out".into(),
            name: "note".into(),
            filtered_name: String::new(),
            track_output: true,
            tick_delay: 20,
            execute_on_first_tick: true,
        });
        let bedrock_protocol_1_26_60::packets::Packet::CommandBlockUpdate(update) = *packet.data
        else {
            panic!("expected a command block update");
        };
        let wire = wire::Encode::encode_to_vec(&*update).expect("encode");
        let expected = [
            &[1][..],     // the block target
            &[4, 119, 0], // zigzag position
            &[1],         // repeat mode
            &[1, 0],      // needs redstone, unconditional
            b"\x06say hi",
            b"\x03out",
            b"\x04note",
            &[0], // no filtered name
            &[1], // track output
            &20i32.to_le_bytes(),
            &[1], // execute on first tick
        ]
        .concat();
        assert_eq!(wire, expected);
    }
}
