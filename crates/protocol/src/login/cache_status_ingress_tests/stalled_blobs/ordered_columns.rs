use super::*;
use bedrock_protocol_1_26_60::packets::BlockActorData;
use bedrock_protocol_1_26_60::types::BlockPos;

/// Builds a full cached column with an inline tail that must survive until both bodies arrive.
fn full_column(x: i32, body: &[u8], biome: &[u8]) -> Packet {
    let mut column = super::super::super::tests::cached_level_chunk(vec![
        crate::client_blob_hash(body),
        crate::client_blob_hash(biome),
    ]);
    column.chunk_position.x = wire::ZigZag32(x);
    column.chunk_position.z = wire::ZigZag32(9);
    column.serialized_chunk_data = Bytes::from_static(b"inline tail");
    Packet::new(column)
}

/// Builds an ordinary block-entity update on the column waiting for cached bodies.
fn block_entity_update(x: i32, y: i32) -> Packet {
    Packet::new(BlockActorData {
        block_position: BlockPos {
            x: wire::ZigZag32(x * 16),
            y: wire::ZigZag32(y),
            z: wire::ZigZag32(9 * 16),
        },
        actor_data_tags: wire::NetworkNbt(Bytes::from_static(&[10, 0, 0])),
    })
}

/// Ordinary updates must not retire healthy cached columns before their bodies arrive.
#[test]
fn cached_columns_keep_metadata_and_order_through_block_entity_updates() {
    let body = b"shared sub-chunk";
    let biome = b"shared biome";
    let mut packets = vec![
        full_column(4, body, biome),
        full_column(7, body, biome),
        cached_reply(4, crate::client_blob_hash(body)),
    ];
    let updates = 16;
    packets.extend((0..updates).map(|y| block_entity_update(4, y)));
    packets.push(Packet::new(SetTime {
        time: wire::ZigZag32(42),
    }));
    let (mut session, send, _expires) = session(&packets, 1);
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::SubChunkReplyAdmission(_)))
    ));
    assert!(
        matches!(
            poll_ingress(&mut session),
            Poll::Ready(WorldIngress::Event(WorldEvent::SetTime(_)))
        ),
        "unrelated progress must preserve the pending columns and their ordered updates"
    );
    assert!(poll_ingress(&mut session).is_pending());
    assert_eq!(session.blob_cache_stats().pending_transactions, 3);
    assert_eq!(
        session.blob_cache_stats().ordinary_ready_events,
        updates as usize
    );
    assert_eq!(session.blob_cache_stats().abandoned_cached_transactions, 0);

    send_packets(&send, &[body_response(body), body_response(biome)]);
    let expected = [body.as_slice(), biome.as_slice(), b"inline tail"].concat();
    for x in [4, 7] {
        let Poll::Ready(WorldIngress::LevelChunk(column, payload)) = poll_ingress(&mut session)
        else {
            panic!("the original cached column must complete before its later updates");
        };
        assert_eq!((column.x, column.z), (x, 9));
        assert_eq!(column.mode, crate::LevelChunkMode::Inline { count: 1 });
        assert_eq!(payload.as_ref(), expected);
    }
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::SubChunks(_)))
    ));
    for y in 0..updates {
        assert!(matches!(
            poll_ingress(&mut session),
            Poll::Ready(WorldIngress::Event(WorldEvent::BlockEntityUpdate(update)))
                if update.position == [4 * 16, y, 9 * 16] && update.nbt == [10, 0, 0]
        ));
    }
    assert!(poll_ingress(&mut session).is_pending());
    let stats = session.blob_cache_stats();
    assert_eq!(stats.reconstructed_level_chunks, 2);
    assert_eq!(stats.reconstructed_sub_chunks, 1);
    assert_eq!(stats.pending_transactions, 0);
    assert_eq!(stats.ordinary_ready_events, 0);
    assert_eq!(stats.recovery_requests, 0);
    assert_eq!(stats.cache_pinned_bytes, 0);
}

/// A decoded-event quota must leave a bounded path to the bodies behind the ordinary burst.
#[test]
fn cached_columns_survive_ordinary_burst_beyond_decoded_lane() {
    let body = b"section body";
    let biome = b"biome body";
    let updates = crate::MAX_CLIENT_BLOB_ORDINARY_READY_EVENTS + 16;
    let mut packets = vec![full_column(4, body, biome)];
    packets.extend((0..updates as i32).map(|y| block_entity_update(4, y)));
    packets.push(Packet::new(SetTime {
        time: wire::ZigZag32(42),
    }));
    let (mut session, send, _expires) = session(&packets, 0);
    // Repeated cancellation retains the full lane, current decoded input and raw suffix.
    for _ in 0..3 {
        assert!(
            poll_ingress(&mut session).is_pending(),
            "ordinary capacity must wait for the requested bodies without retiring their metadata"
        );
    }
    assert_eq!(
        session.blob_cache_stats().ordinary_ready_events,
        crate::MAX_CLIENT_BLOB_ORDINARY_READY_EVENTS
    );
    send_packets(&send, &[body_response(body), body_response(biome)]);
    let Poll::Ready(WorldIngress::LevelChunk(column, payload)) = poll_ingress(&mut session) else {
        panic!("requested bodies behind raw ordinary input must complete the original column");
    };
    assert_eq!(column.mode, crate::LevelChunkMode::Inline { count: 1 });
    assert_eq!(
        payload.as_ref(),
        [body.as_slice(), biome.as_slice(), b"inline tail"].concat()
    );
    for y in 0..updates as i32 {
        assert!(
            matches!(poll_ingress(&mut session), Poll::Ready(WorldIngress::Event(WorldEvent::BlockEntityUpdate(update))) if update.position[1] == y)
        );
    }
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::SetTime(_)))
    ));
    assert!(poll_ingress(&mut session).is_pending());
    assert_eq!(session.blob_cache_stats().abandoned_cached_transactions, 0);
    assert_eq!(session.blob_cache_stats().recovery_requests, 0);
}

/// A ready multi-column reply carries the earlier column's ordering dependency to later updates.
#[test]
fn cached_columns_preserve_transitively_blocked_ordinary_work_without_spinning() {
    let body = b"resident section";
    let biome = b"missing biome";
    let mut reply = cached_reply(4, crate::client_blob_hash(body));
    let packets::Packet::SubChunk(reply_data) = &mut *reply.data else {
        unreachable!();
    };
    let mut other = reply_data.sub_chunk_data[0].clone();
    other.sub_chunk_pos_offset.subchunk_offset_x = wire::I8(3);
    reply_data.sub_chunk_data.push(other);
    let (mut session, send, _expires) = session(
        &[
            full_column(7, body, biome),
            reply,
            block_entity_update(4, 0),
            // Later ready terrain must not overtake the earlier update on its other column.
            full_column(4, body, body),
        ],
        0,
    );
    session
        .blob_cache
        .as_ref()
        .unwrap()
        .cache()
        .insert(body)
        .unwrap();
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::SubChunkReplyAdmission(_)))
    ));
    for _ in 0..3 {
        assert!(poll_ingress(&mut session).is_pending());
    }
    assert_eq!(session.blob_cache_stats().abandoned_cached_transactions, 0);
    send_packets(&send, &[body_response(biome)]);
    assert!(
        matches!(poll_ingress(&mut session), Poll::Ready(WorldIngress::LevelChunk(column, _)) if column.x == 7)
    );
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::SubChunks(_)))
    ));
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::BlockEntityUpdate(_)))
    ));
    assert!(
        matches!(poll_ingress(&mut session), Poll::Ready(WorldIngress::LevelChunk(column, _)) if column.x == 4)
    );
    assert!(poll_ingress(&mut session).is_pending());
}

/// A boundary beyond the full decoded lane drains recovery and older updates before resetting.
#[test]
fn cached_column_recovery_waits_for_ordinary_event_capacity() {
    let mut packets = vec![full_column(4, b"body", b"biome")];
    let updates = crate::MAX_CLIENT_BLOB_ORDINARY_READY_EVENTS;
    packets.extend((0..updates as i32).map(|y| block_entity_update(4, y)));
    packets.push(Packet::new(SetTime {
        time: wire::ZigZag32(42),
    }));
    packets.push(Packet::new(
        bedrock_protocol_1_26_60::packets::ChangeDimension::default(),
    ));
    let (mut session, _send, _expires) = session(&packets, 0);
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::ChunkResync(_)))
    ));
    assert_eq!(session.blob_cache_stats().ordinary_ready_events, updates);
    for y in 0..updates as i32 {
        assert!(matches!(
            poll_ingress(&mut session),
            Poll::Ready(WorldIngress::Event(WorldEvent::BlockEntityUpdate(update)))
                if update.position[1] == y
        ));
    }
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::SetTime(_)))
    ));
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::ChangeDimension(_)))
    ));
    assert!(poll_ingress(&mut session).is_pending());
    assert_eq!(session.blob_cache_stats().abandoned_cached_transactions, 1);
    assert_eq!(session.blob_cache_stats().ordinary_backpressure, 0);
}

/// A decoded dimension boundary drains the full lane even if no more old-world bodies arrive.
#[test]
fn cached_column_recovery_reaches_retained_dimension_boundary() {
    assert_dimension_boundary_progress(crate::MAX_CLIENT_BLOB_ORDINARY_READY_EVENTS);
}

/// A dimension change must not overtake and discard earlier blocked updates below the quota.
#[test]
fn cached_column_recovery_orders_dimension_boundary_before_ordinary_capacity() {
    assert_dimension_boundary_progress(1);
}

/// Checks boundary progress, old-update ordering and epoch isolation with or without late bodies.
fn assert_dimension_boundary_progress(updates: usize) {
    for late_bodies in [false, true] {
        let mut packets = vec![full_column(4, b"body", b"biome")];
        packets.extend((0..updates as i32).map(|y| block_entity_update(4, y)));
        packets.push(Packet::new(
            bedrock_protocol_1_26_60::packets::ChangeDimension::default(),
        ));
        if late_bodies {
            packets.extend([body_response(b"body"), body_response(b"biome")]);
        }
        let (mut session, send, _expires) = session(&packets, 0);
        assert!(
            matches!(
                poll_ingress(&mut session),
                Poll::Ready(WorldIngress::Event(WorldEvent::ChunkResync(_)))
            ),
            "a retained dimension boundary must recover without reading later bodies or waiting for input"
        );
        assert_eq!(session.blob_cache_stats().ordinary_ready_events, updates);
        for y in 0..updates as i32 {
            assert!(matches!(
                poll_ingress(&mut session),
                Poll::Ready(WorldIngress::Event(WorldEvent::BlockEntityUpdate(update)))
                    if update.position[1] == y
            ));
        }
        assert!(matches!(
            poll_ingress(&mut session),
            Poll::Ready(WorldIngress::Event(WorldEvent::ChangeDimension(_)))
        ));
        send_packets(
            &send,
            &[Packet::new(SetTime {
                time: wire::ZigZag32(42),
            })],
        );
        assert!(matches!(
            poll_ingress(&mut session),
            Poll::Ready(WorldIngress::Event(WorldEvent::SetTime(time))) if time.time == 42
        ));
        assert!(poll_ingress(&mut session).is_pending());
        let stats = session.blob_cache_stats();
        assert_eq!(stats.abandoned_cached_transactions, 1);
        assert_eq!(stats.reconstructed_level_chunks, 0);
        assert_eq!(stats.pending_transactions, 0);
        assert_eq!(stats.ordinary_ready_events, 0);
        assert_eq!(stats.cache_pinned_bytes, 0);
        assert_eq!(stats.ordinary_backpressure, 0);
    }
}

/// Byte pressure follows the same recovery path without losing the current decoded item.
#[test]
fn cached_column_recovery_preserves_input_at_ordinary_byte_capacity() {
    let (mut session, send, _expires) = session(&[full_column(4, b"body", b"biome")], 0);
    assert!(poll_ingress(&mut session).is_pending());
    let update = WorldEvent::BlockUpdates(vec![crate::BlockUpdateEvent {
        dimension: 0,
        position: [4 * 16, 0, 9 * 16],
        layer: 0,
        network_id: 1,
    }]);
    let half = crate::MAX_CLIENT_BLOB_ORDINARY_READY_BYTES / 2;
    session
        .blob_cache
        .as_mut()
        .unwrap()
        .accept_world_event(update, half)
        .unwrap();
    session.pending_ordinary = Some((
        WorldIngress::Event(WorldEvent::SetTime(crate::SetTimeEvent { time: 42 })),
        half,
    ));
    send_packets(
        &send,
        &[Packet::new(
            bedrock_protocol_1_26_60::packets::ChangeDimension::default(),
        )],
    );
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::ChunkResync(_)))
    ));
    assert!(session.pending_ordinary.is_some());
    assert!(
        session.blob_cache_stats().ordinary_ready_bytes
            <= crate::MAX_CLIENT_BLOB_ORDINARY_READY_BYTES
    );
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::BlockUpdates(_)))
    ));
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::SetTime(_)))
    ));
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::ChangeDimension(_)))
    ));
    assert!(poll_ingress(&mut session).is_pending());
    assert!(session.pending_ordinary.is_none());
    assert_eq!(session.blob_cache_stats().ordinary_ready_bytes, 0);
    assert_eq!(session.blob_cache_stats().ordinary_backpressure, 0);
}
