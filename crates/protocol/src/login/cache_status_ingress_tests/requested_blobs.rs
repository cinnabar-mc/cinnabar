use super::*;
use bedrock_protocol_1_26_60::enums::SubChunkRequestResult;
use bedrock_protocol_1_26_60::packets::{ClientCacheMissResponse, SubChunk, UpdateBlock};
use bedrock_protocol_1_26_60::types::{
    BlockPos, DimensionType, MissingBlobData, SubChunkData, SubChunkPos, SubChunkPosOffset,
};

/// Polls buffered ingress without waiting for transport input or status writes.
fn next_ingress(session: &mut PlaySession<impl bridge::FrameRead>) -> WorldIngress {
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    let mut receive = std::pin::pin!(session.recv_world_ingress_with_blob_cache(0));
    let Poll::Ready(Ok(event)) = receive.as_mut().poll(&mut cx) else {
        panic!("buffered ingress must be ready");
    };
    event
}

/// Interleaved ordinary updates preserve both cached replies until their bodies are usable.
#[tokio::test]
async fn requested_blob_survives_repeated_ordinary_update_interleave() {
    let payload = Bytes::from_static(b"requested terrain");
    let hash = crate::client_blob_hash(&payload);
    let reply = Packet::new(SubChunk {
        cache_enabled: true,
        dimension_type: DimensionType(0),
        center_pos: SubChunkPos {
            subchunk_position_x: wire::I32LE(4),
            subchunk_position_y: wire::I32LE(-3),
            subchunk_position_z: wire::I32LE(9),
        },
        sub_chunk_data: vec![SubChunkData {
            sub_chunk_pos_offset: SubChunkPosOffset::default(),
            sub_chunk_request_result: SubChunkRequestResult::Success,
            serialized_sub_chunk: Some(Bytes::new()),
            blob_id: Some(wire::U64LE(hash)),
            ..Default::default()
        }],
    });
    let update = |x| {
        Packet::new(UpdateBlock {
            block_position: BlockPos {
                x: wire::ZigZag32(x * 16),
                y: wire::ZigZag32(0),
                z: wire::ZigZag32(9 * 16),
            },
            block_runtime_id: wire::VarUInt(1),
            ..Default::default()
        })
    };
    let response = Packet::new(ClientCacheMissResponse {
        missing_blobs: vec![MissingBlobData {
            blob_id: wire::U64LE(hash),
            blob_data: payload.clone(),
        }],
    });
    let startup: Vec<_> = [
        reply.clone(),
        update(20),
        update(4),
        response.clone(),
        reply,
        update(4),
        response,
        Packet::new(SetTime {
            time: wire::ZigZag32(42),
        }),
    ]
    .iter()
    .map(crate::session_transport::tests::packet_bytes)
    .collect();
    let (reader, frames, _writer) = bridge::tasks::prepare_connection(
        tokio::io::empty(),
        tokio::io::sink(),
        bridge::MAX_FRAME_LEN,
    );
    let cache = ClientBlobCache::default();
    let transport = SessionTransport::new(reader, frames, &startup);
    let mut session = PlaySession::new(transport, Some(cache.clone()), 1, false);

    assert!(matches!(
        next_ingress(&mut session),
        WorldIngress::Event(WorldEvent::SubChunkReplyAdmission(_))
    ));
    assert!(matches!(
        next_ingress(&mut session),
        WorldIngress::Event(WorldEvent::BlockUpdates(updates))
            if updates[0].position[0] == 20 * 16
    ));
    assert!(matches!(
        next_ingress(&mut session),
        WorldIngress::Event(WorldEvent::SubChunks(_))
    ));
    assert!(matches!(
        next_ingress(&mut session),
        WorldIngress::Event(WorldEvent::BlockUpdates(updates))
            if updates[0].position[0] == 4 * 16
    ));
    assert!(matches!(
        next_ingress(&mut session),
        WorldIngress::Event(WorldEvent::SubChunkReplyAdmission(_))
    ));
    let WorldIngress::Event(WorldEvent::SubChunks(batch)) = next_ingress(&mut session) else {
        panic!("the second reply must publish terrain before its later update");
    };
    assert_eq!(batch.entries.len(), 1);
    assert_eq!(batch.entries[0].position, [4, -3, 9]);
    assert_eq!(
        batch.entries[0].result,
        crate::SubChunkResult::Success {
            payload: payload.to_vec(),
        }
    );
    assert!(matches!(
        next_ingress(&mut session),
        WorldIngress::Event(WorldEvent::BlockUpdates(_))
    ));
    assert!(matches!(
        next_ingress(&mut session),
        WorldIngress::Event(WorldEvent::SetTime(_))
    ));
    assert!(cache.contains(hash));
    let stats = session.blob_cache_stats();
    assert_eq!(stats.abandoned_cached_transactions, 0);
    assert_eq!(stats.reconstructed_sub_chunks, 2);
    assert_eq!(stats.skipped_miss_responses, 0);
    assert_eq!(stats.pending_transactions, 0);
}
