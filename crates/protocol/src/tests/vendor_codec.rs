use bedrock_protocol_1_26_60::packets::{
    ClientboundDataDrivenUIReload, MapInfoRequest, NetworkChunkPublisherUpdate,
};
use bedrock_protocol_1_26_60::types::{ChunkPos, PixelRequest};
use bedrock_protocol_1_26_60::wire::{self, Decode, Encode};

/// Empty elements need no payload bytes, but their count still obeys reader limits.
#[test]
fn zero_byte_collection_elements_decode_under_the_field_limit() {
    let bytes = [0x80, 0x08];
    let field = wire::FieldId(0);
    let mut reader = wire::Reader::new(&bytes);
    let values =
        wire::decode_collection::<ClientboundDataDrivenUIReload>(&mut reader, field, 0).unwrap();
    assert_eq!(values, vec![ClientboundDataDrivenUIReload {}; 1_024]);
    assert_eq!(reader.remaining(), 0);

    let limits = wire::Limits::unbounded().with_field(field, values.len() - 1);
    assert!(matches!(
        wire::decode_collection::<ClientboundDataDrivenUIReload>(
            &mut wire::Reader::new(&bytes).with_limits(&limits),
            field,
            0,
        ),
        Err(wire::DecodeError::LengthLimitExceeded {
            actual: 1_024,
            limit: 1_023,
            ..
        })
    ));
}

/// Map pixel counts use a little-endian u32 and enforce the schema maximum on decode.
#[test]
fn map_pixel_counts_enforce_the_fixed_width_schema_bound() {
    let packet = MapInfoRequest {
        client_pixels_list: vec![PixelRequest::default(); 16_384],
        ..Default::default()
    };
    let bytes = packet.encode_to_vec().unwrap();
    assert_eq!(MapInfoRequest::decode_exact(&bytes).unwrap(), packet);

    let mut oversized = Vec::new();
    let mut writer = wire::Writer::new(&mut oversized);
    packet.map_unique_id.encode(&mut writer).unwrap();
    wire::encode_collection_u32le(
        &mut writer,
        &vec![PixelRequest::default(); packet.client_pixels_list.len() + 1],
    )
    .unwrap();
    assert!(matches!(
        MapInfoRequest::decode_exact(&oversized),
        Err(wire::DecodeError::SchemaConstraint("length above maximum"))
    ));
}

/// Published chunk counts apply the same schema bound on encode and decode.
#[test]
fn published_chunk_counts_enforce_the_fixed_width_schema_bound() {
    let packet = NetworkChunkPublisherUpdate {
        server_built_chunks_list: vec![ChunkPos::default(); 9_216],
        ..Default::default()
    };
    let bytes = packet.encode_to_vec().unwrap();
    assert_eq!(
        NetworkChunkPublisherUpdate::decode_exact(&bytes).unwrap(),
        packet
    );

    let mut oversized = Vec::new();
    let mut writer = wire::Writer::new(&mut oversized);
    packet.new_position_for_view.encode(&mut writer).unwrap();
    packet.new_radius_for_view.encode(&mut writer).unwrap();
    wire::encode_collection_u32le(
        &mut writer,
        &vec![ChunkPos::default(); packet.server_built_chunks_list.len() + 1],
    )
    .unwrap();
    assert!(matches!(
        NetworkChunkPublisherUpdate::decode_exact(&oversized),
        Err(wire::DecodeError::SchemaConstraint("length above maximum"))
    ));
}
