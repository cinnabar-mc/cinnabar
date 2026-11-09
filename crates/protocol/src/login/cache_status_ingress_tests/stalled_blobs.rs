use super::*;
use bedrock_protocol_1_26_60::enums::SubChunkRequestResult;
use bedrock_protocol_1_26_60::packets::SubChunk;
use bedrock_protocol_1_26_60::types::{DimensionType, SubChunkData, SubChunkPos};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::pin::Pin;
use tokio::sync::{mpsc, oneshot};

mod ordered_columns;

thread_local! {
    static TIMERS: RefCell<VecDeque<oneshot::Receiver<()>>> = const { RefCell::new(VecDeque::new()) };
}

/// Delivers complete frames only when the test explicitly supplies them.
struct ControlledFrames(mpsc::UnboundedReceiver<Bytes>);

impl futures::Stream for ControlledFrames {
    type Item = Result<Bytes, bridge::BridgeError>;

    /// Retains an idle connection until the test queues a frame or closes it.
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().0.poll_recv(cx).map(|frame| frame.map(Ok))
    }
}

impl bridge::FrameRead for ControlledFrames {
    /// These synthetic frames have no latency trace connection.
    fn trace_conn(&self) -> u64 {
        0
    }

    /// The fixture does not record latency trace identities.
    fn set_trace_conn(&self, _: u64) {}
}

/// Builds a cached reply for one requested height with a missing body.
fn cached_reply(x: i32, hash: u64) -> Packet {
    Packet::new(SubChunk {
        cache_enabled: true,
        dimension_type: DimensionType(0),
        center_pos: SubChunkPos {
            subchunk_position_x: wire::I32LE(x),
            subchunk_position_y: wire::I32LE(-3),
            subchunk_position_z: wire::I32LE(9),
        },
        sub_chunk_data: vec![SubChunkData {
            sub_chunk_request_result: SubChunkRequestResult::Success,
            serialized_sub_chunk: Some(Bytes::new()),
            blob_id: Some(wire::U64LE(hash)),
            ..Default::default()
        }],
    })
}

/// Polls one receive without a runtime clock or waiting for input.
fn poll_ingress(session: &mut PlaySession<ControlledFrames>) -> Poll<WorldIngress> {
    let mut cx = Context::from_waker(std::task::Waker::noop());
    let mut receive = std::pin::pin!(session.recv_world_ingress_with_blob_cache(0));
    receive.as_mut().poll(&mut cx).map(Result::unwrap)
}

/// Creates an idle session with host deadlines controlled by explicit completion signals.
fn session(
    startup: &[Packet],
    timer_count: usize,
) -> (
    PlaySession<ControlledFrames>,
    mpsc::UnboundedSender<Bytes>,
    Vec<oneshot::Sender<()>>,
) {
    let mut expires = Vec::new();
    TIMERS.with(|timers| {
        let mut timers = timers.borrow_mut();
        timers.clear();
        for _ in 0..timer_count {
            let (expire, expired) = oneshot::channel();
            expires.push(expire);
            timers.push_back(expired);
        }
    });
    let timers = bridge::tasks::Timers(|_| {
        let expired = TIMERS.with(|timers| timers.borrow_mut().pop_front().expect("host timer"));
        Box::pin(async move { expired.await.expect("explicit expiry") })
    });
    let (_, frames, _writer) = bridge::tasks::prepare_connection(
        tokio::io::empty(),
        tokio::io::sink(),
        bridge::MAX_FRAME_LEN,
    );
    let (send, recv) = mpsc::unbounded_channel();
    let startup: Vec<_> = startup
        .iter()
        .map(crate::session_transport::tests::packet_bytes)
        .collect();
    let transport = SessionTransport::with_timers(ControlledFrames(recv), frames, &startup, timers);
    (
        PlaySession::new(transport, Some(ClientBlobCache::default()), 1, false),
        send,
        expires,
    )
}

/// An admitted reply must recover even when no more transport input arrives.
#[test]
fn stalled_cached_reply_recovers_on_host_deadline_without_input() {
    let hash = crate::client_blob_hash(b"missing terrain");
    let (mut session, _send, mut expires) = session(&[cached_reply(4, hash)], 1);
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::SubChunkReplyAdmission(_)))
    ));
    assert!(poll_ingress(&mut session).is_pending());
    expires.remove(0).send(()).unwrap();
    assert!(
        matches!(
            poll_ingress(&mut session),
            Poll::Ready(WorldIngress::Event(WorldEvent::ChunkResync(event)))
                if (event.x, event.z) == (4, 9)
                    && event.requested_sub_chunk_ys == Some(vec![-3])
        ),
        "a missing body must retain a recovery owner after reply admission"
    );
}

/// Sends one synthetic batch without depending on a socket writer.
fn send_packets(send: &mpsc::UnboundedSender<Bytes>, packets: &[Packet]) {
    send.send(crate::session_transport::batch_frame(packets).unwrap())
        .unwrap();
}

/// Completes one missing body with its actual verified hash.
fn body_response(body: &[u8]) -> Packet {
    Packet::new(bedrock_protocol_1_26_60::packets::ClientCacheMissResponse {
        missing_blobs: vec![bedrock_protocol_1_26_60::types::MissingBlobData {
            blob_id: wire::U64LE(crate::client_blob_hash(body)),
            blob_data: Bytes::copy_from_slice(body),
        }],
    })
}

/// Checks that intake returned exactly the expected height for a recovered column.
fn assert_recovery(session: &mut PlaySession<ControlledFrames>, x: i32) {
    assert!(matches!(
        poll_ingress(session),
        Poll::Ready(WorldIngress::Event(WorldEvent::ChunkResync(event)))
            if (event.x, event.z) == (x, 9)
                && event.requested_sub_chunk_ys == Some(vec![-3])
    ));
}

/// No cancelled receive may restart the only deadline protecting a missing body.
#[test]
fn stalled_cached_reply_deadline_survives_cancelled_receives() {
    let hash = crate::client_blob_hash(b"missing terrain");
    let (mut session, _send, mut expires) = session(&[cached_reply(4, hash)], 1);
    assert!(poll_ingress(&mut session).is_ready());
    for _ in 0..8 {
        assert!(poll_ingress(&mut session).is_pending());
    }
    expires.remove(0).send(()).unwrap();
    assert_recovery(&mut session, 4);
    assert_eq!(session.blob_cache_stats().abandoned_cached_transactions, 1);
    assert!(poll_ingress(&mut session).is_pending());
}

/// Late bodies warm the cache after expiry and publish terrain only on a fresh reply.
#[test]
fn stalled_cached_reply_keeps_late_body_usable_without_stale_publication() {
    let body = b"late terrain";
    let hash = crate::client_blob_hash(body);
    let reply = cached_reply(4, hash);
    let (mut session, send, mut expires) = session(std::slice::from_ref(&reply), 1);
    assert!(poll_ingress(&mut session).is_ready());
    assert!(poll_ingress(&mut session).is_pending());
    expires.remove(0).send(()).unwrap();
    assert_recovery(&mut session, 4);
    send_packets(&send, &[body_response(body)]);
    assert!(poll_ingress(&mut session).is_pending());
    assert!(session.blob_cache.as_ref().unwrap().cache().contains(hash));
    send_packets(&send, &[reply]);
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::SubChunkReplyAdmission(_)))
    ));
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::SubChunks(batch)))
            if batch.entries[0].position == [4, -3, 9]
                && batch.entries[0].result == crate::SubChunkResult::Success { payload: body.to_vec() }
    ));
    assert_eq!(session.blob_cache_stats().skipped_miss_responses, 0);
    assert_eq!(session.blob_cache_stats().cache_pinned_bytes, 0);
}

/// Expiry releases hash ownership so a recovery requests the body again and remains recoverable.
#[test]
fn stalled_cached_reply_rerequests_the_missing_body_after_each_expiry() {
    let hash = crate::client_blob_hash(b"missing terrain");
    let reply = cached_reply(4, hash);
    let (mut session, send, mut expires) = session(std::slice::from_ref(&reply), 2);
    assert!(poll_ingress(&mut session).is_ready());
    assert!(poll_ingress(&mut session).is_pending());
    expires.remove(0).send(()).unwrap();
    assert_recovery(&mut session, 4);
    send_packets(&send, &[reply]);
    assert!(poll_ingress(&mut session).is_ready());
    let status = session
        .cache_status
        .lock()
        .unwrap()
        .select(Default::default())
        .unwrap();
    assert_eq!(status.missing_ids, [wire::U64LE(hash), wire::U64LE(hash)]);
    assert!(poll_ingress(&mut session).is_pending());
    expires.remove(0).send(()).unwrap();
    assert_recovery(&mut session, 4);
}

/// New admissions cannot renew an older deadline or be retired by that older deadline.
#[test]
fn stalled_cached_reply_newer_transactions_receive_a_separate_interval() {
    let hash = crate::client_blob_hash(b"missing terrain");
    let (mut session, send, mut expires) = session(&[cached_reply(4, hash)], 2);
    assert!(poll_ingress(&mut session).is_ready());
    assert!(poll_ingress(&mut session).is_pending());
    send_packets(&send, &[cached_reply(5, hash)]);
    assert!(poll_ingress(&mut session).is_ready());
    expires.remove(0).send(()).unwrap();
    assert_recovery(&mut session, 4);
    assert_eq!(session.blob_cache_stats().pending_transactions, 1);
    assert!(poll_ingress(&mut session).is_pending());
    expires.remove(0).send(()).unwrap();
    assert_recovery(&mut session, 5);
    assert_eq!(session.blob_cache_stats().pending_transactions, 0);
}

/// An expired shared-hash owner must leave a newer waiter able to reconstruct normally.
#[test]
fn stalled_cached_reply_preserves_newer_shared_hash_waiters() {
    let body = b"shared terrain";
    let hash = crate::client_blob_hash(body);
    let (mut session, send, mut expires) = session(&[cached_reply(4, hash)], 2);
    assert!(poll_ingress(&mut session).is_ready());
    assert!(poll_ingress(&mut session).is_pending());
    send_packets(&send, &[cached_reply(5, hash)]);
    assert!(poll_ingress(&mut session).is_ready());
    expires.remove(0).send(()).unwrap();
    assert_recovery(&mut session, 4);
    send_packets(&send, &[body_response(body)]);
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::SubChunks(batch)))
            if batch.entries[0].position == [5, -3, 9]
    ));
    assert!(poll_ingress(&mut session).is_pending());
    assert!(
        expires.remove(0).send(()).is_err(),
        "resolved work releases its timer"
    );
}

/// Fresh terrain replies must fetch a timed-out shared body even while a newer reply waits.
#[test]
fn stalled_cached_reply_retry_fetches_hash_still_needed_by_newer_waiters() {
    let body = b"shared retry body";
    let hash = crate::client_blob_hash(body);
    let (mut session, send, mut expires) = session(&[cached_reply(4, hash)], 2);
    assert!(poll_ingress(&mut session).is_ready());
    assert!(poll_ingress(&mut session).is_pending());
    send_packets(&send, &[cached_reply(5, hash)]);
    assert!(poll_ingress(&mut session).is_ready());
    expires.remove(0).send(()).unwrap();
    assert_recovery(&mut session, 4);
    send_packets(&send, &[cached_reply(4, hash)]);
    assert!(poll_ingress(&mut session).is_ready());
    let status = session
        .cache_status
        .lock()
        .unwrap()
        .select(Default::default())
        .unwrap();
    assert_eq!(status.missing_ids, [wire::U64LE(hash), wire::U64LE(hash)]);
    send_packets(&send, &[body_response(body)]);
    for x in [5, 4] {
        assert!(matches!(
            poll_ingress(&mut session),
            Poll::Ready(WorldIngress::Event(WorldEvent::SubChunks(batch)))
                if batch.entries[0].position == [x, -3, 9]
                    && batch.entries[0].result == crate::SubChunkResult::Success { payload: body.to_vec() }
        ));
    }
}

/// A full-column dependency retains its payload while fresh SubChunk recovery fetches their shared body.
#[test]
fn stalled_cached_reply_shared_full_column_does_not_suppress_retry_fetch() {
    let body = b"shared full-column body";
    let hash = crate::client_blob_hash(body);
    let level = Packet::new(super::super::tests::cached_level_chunk(vec![hash]));
    let (mut session, send, mut expires) = session(&[level, cached_reply(4, hash)], 2);
    assert!(poll_ingress(&mut session).is_ready());
    assert!(poll_ingress(&mut session).is_pending());
    expires.remove(0).send(()).unwrap();
    assert_recovery(&mut session, 4);
    send_packets(&send, &[cached_reply(4, hash)]);
    assert!(poll_ingress(&mut session).is_ready());
    let status = session
        .cache_status
        .lock()
        .unwrap()
        .select(Default::default())
        .unwrap();
    assert_eq!(status.missing_ids, [wire::U64LE(hash), wire::U64LE(hash)]);
    send_packets(&send, &[body_response(body)]);
    assert!(
        matches!(poll_ingress(&mut session), Poll::Ready(WorldIngress::LevelChunk(_, payload)) if payload.as_ref() == body)
    );
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::SubChunks(batch)))
            if batch.entries[0].position == [4, -3, 9]
    ));
}

/// Later reconstructed terrain in the same column remains ordered behind exact rollback.
#[test]
fn stalled_cached_reply_recovery_precedes_ready_same_column_terrain() {
    let hash = crate::client_blob_hash(b"missing terrain");
    let (mut session, send, mut expires) = session(&[cached_reply(4, hash)], 1);
    let resident = session
        .blob_cache
        .as_ref()
        .unwrap()
        .cache()
        .insert(b"new terrain")
        .unwrap();
    assert!(poll_ingress(&mut session).is_ready());
    assert!(poll_ingress(&mut session).is_pending());
    send_packets(&send, &[cached_reply(4, resident)]);
    assert!(poll_ingress(&mut session).is_ready());
    assert!(poll_ingress(&mut session).is_pending());
    expires.remove(0).send(()).unwrap();
    assert_recovery(&mut session, 4);
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::SubChunks(batch)))
            if batch.entries[0].position == [4, -3, 9]
                && batch.entries[0].result == crate::SubChunkResult::Success { payload: b"new terrain".to_vec() }
    ));
    assert_eq!(session.blob_cache_stats().abandoned_cached_transactions, 1);
}

/// A session reset cancels the old timer before transaction sequence IDs are reused.
#[test]
fn stalled_cached_reply_reset_cancels_the_previous_deadline() {
    let hash = crate::client_blob_hash(b"missing terrain");
    let (mut session, send, mut expires) = session(&[cached_reply(4, hash)], 2);
    assert!(poll_ingress(&mut session).is_ready());
    assert!(poll_ingress(&mut session).is_pending());
    session.reset_blob_cache_pending();
    assert!(expires.remove(0).send(()).is_err());
    send_packets(&send, &[cached_reply(5, hash)]);
    assert!(poll_ingress(&mut session).is_ready());
    assert!(poll_ingress(&mut session).is_pending());
    expires.remove(0).send(()).unwrap();
    assert_recovery(&mut session, 5);
}

/// A core disconnect cancels recovery without requiring another world receive.
#[test]
fn stalled_cached_reply_disconnect_cancels_the_previous_deadline() {
    let hash = crate::client_blob_hash(b"missing terrain");
    let (mut session, send, mut expires) = session(&[cached_reply(4, hash)], 1);
    assert!(poll_ingress(&mut session).is_ready());
    assert!(poll_ingress(&mut session).is_pending());
    let mut terminal = vec![bridge::contract::KIND_DISCONNECT];
    terminal.extend_from_slice(
        br#"{"reason":0,"message":"fixture","filtered_message":"","hide_screen":false}"#,
    );
    send.send(Bytes::from(terminal)).unwrap();
    {
        let mut cx = Context::from_waker(std::task::Waker::noop());
        let mut receive = std::pin::pin!(session.recv_world_ingress_with_blob_cache(0));
        assert!(matches!(
            receive.as_mut().poll(&mut cx),
            Poll::Ready(Err(ProtocolError::SessionBoundary))
        ));
    }
    assert!(expires.remove(0).send(()).is_err());
}

/// A fast transfer gives the replacement backend its own full recovery interval.
#[test]
fn stalled_cached_reply_fast_transfer_retires_the_previous_deadline() {
    let hash = crate::client_blob_hash(b"missing terrain");
    let (mut session, send, mut expires) = session(&[cached_reply(4, hash)], 2);
    assert!(poll_ingress(&mut session).is_ready());
    assert!(poll_ingress(&mut session).is_pending());
    session.arm_blob_cache_reset_for_fast_transfer();
    send_packets(&send, &[cached_reply(5, hash)]);
    assert_recovery(&mut session, 4);
    assert!(expires.remove(0).send(()).is_err());
    assert!(poll_ingress(&mut session).is_ready());
    assert!(poll_ingress(&mut session).is_pending());
    expires.remove(0).send(()).unwrap();
    assert_recovery(&mut session, 5);
}

/// Ordinary traffic does not postpone a missing body's recovery opportunity.
#[test]
fn stalled_cached_reply_recovers_ahead_of_unrelated_ready_input() {
    let hash = crate::client_blob_hash(b"missing terrain");
    let (mut session, send, mut expires) = session(&[cached_reply(4, hash)], 1);
    assert!(poll_ingress(&mut session).is_ready());
    assert!(poll_ingress(&mut session).is_pending());
    let time = Packet::new(SetTime {
        time: wire::ZigZag32(42),
    });
    send_packets(&send, std::slice::from_ref(&time));
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::SetTime(_)))
    ));
    send_packets(&send, &[time]);
    expires.remove(0).send(()).unwrap();
    assert_recovery(&mut session, 4);
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::SetTime(_)))
    ));
}

/// Slow full columns retain all cached blocks, biomes and inline tails through another reply's deadline.
#[test]
fn stalled_cached_level_chunk_preserves_complete_payload_after_deadline() {
    for inline in [false, true] {
        // One uniform network block storage, one uniform biome storage and no border blocks.
        let blocks = &[9, 1, 0, 1, 14];
        let biomes = &[1, 84];
        let tail = &[0];
        let hashes: Vec<_> = if inline {
            vec![blocks.as_slice(), biomes.as_slice()]
        } else {
            vec![biomes.as_slice()]
        }
        .into_iter()
        .map(crate::client_blob_hash)
        .collect();
        let mut level = super::super::tests::cached_level_chunk(hashes);
        if !inline {
            level.client_request_sub_chunk_limit = Some(wire::ZigZag32(-1));
        }
        level.serialized_chunk_data = Bytes::copy_from_slice(tail);
        let (mut session, send, mut expires) = session(
            &[
                Packet::new(level),
                cached_reply(4, crate::client_blob_hash(b"other missing body")),
            ],
            2,
        );
        assert!(matches!(
            poll_ingress(&mut session),
            Poll::Ready(WorldIngress::Event(WorldEvent::SubChunkReplyAdmission(_)))
        ));
        assert!(poll_ingress(&mut session).is_pending());
        expires.remove(0).send(()).unwrap();
        assert!(matches!(
            poll_ingress(&mut session),
            Poll::Ready(WorldIngress::Event(WorldEvent::ChunkResync(_)))
        ));
        let mut responses = Vec::new();
        if inline {
            responses.push(body_response(blocks));
        }
        responses.push(body_response(biomes));
        responses.push(Packet::new(SetTime {
            time: wire::ZigZag32(42),
        }));
        send_packets(&send, &responses);
        let Poll::Ready(WorldIngress::LevelChunk(event, payload)) = poll_ingress(&mut session)
        else {
            panic!("late full-column bodies must still install the original complete column");
        };
        let expected = if inline {
            [blocks.as_slice(), biomes.as_slice(), tail.as_slice()].concat()
        } else {
            [biomes.as_slice(), tail.as_slice()].concat()
        };
        assert_eq!(payload.as_ref(), expected);
        assert_eq!((event.x, event.z), (0, 0));
        assert_eq!(
            event.mode,
            if inline {
                crate::LevelChunkMode::Inline { count: 1 }
            } else {
                crate::LevelChunkMode::LimitlessRequests
            }
        );
        assert_eq!(session.blob_cache_stats().reconstructed_level_chunks, 1);
        assert_eq!(session.blob_cache_stats().cache_pinned_bytes, 0);
        assert!(session.blob_cache_stats().pending_bytes <= crate::MAX_CLIENT_BLOB_PENDING_BYTES);
        assert!(matches!(
            poll_ingress(&mut session),
            Poll::Ready(WorldIngress::Event(WorldEvent::SetTime(_)))
        ));
    }
}

/// Later same-column terrain waits for the original full-column payload across another timeout.
#[test]
fn stalled_cached_level_chunk_keeps_later_same_column_terrain_ordered() {
    let biomes = &[1, 84];
    let biome_hash = crate::client_blob_hash(biomes);
    let mut level = super::super::tests::cached_level_chunk(vec![biome_hash]);
    level.chunk_position.x = wire::ZigZag32(4);
    level.chunk_position.z = wire::ZigZag32(9);
    level.client_request_sub_chunk_limit = Some(wire::ZigZag32(-1));
    level.serialized_chunk_data = Bytes::from_static(&[0]);
    let (mut session, send, mut expires) = session(
        &[
            Packet::new(level),
            cached_reply(5, crate::client_blob_hash(b"missing body")),
        ],
        1,
    );
    let resident = session
        .blob_cache
        .as_ref()
        .unwrap()
        .cache()
        .insert(b"later terrain")
        .unwrap();
    assert!(poll_ingress(&mut session).is_ready());
    send_packets(&send, &[cached_reply(4, resident)]);
    assert!(poll_ingress(&mut session).is_ready());
    expires.remove(0).send(()).unwrap();
    assert_recovery(&mut session, 5);
    assert!(poll_ingress(&mut session).is_pending());
    send_packets(&send, &[body_response(biomes)]);
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::LevelChunk(event, payload))
            if (event.x, event.z) == (4, 9) && payload.as_ref() == [1, 84, 0]
    ));
    assert!(matches!(
        poll_ingress(&mut session),
        Poll::Ready(WorldIngress::Event(WorldEvent::SubChunks(batch)))
            if batch.entries[0].position == [4, -3, 9]
    ));
}

/// Leaves one disk read pending so the test can expire an earlier network dependency.
#[derive(Debug)]
struct PendingRead(u64);

impl crate::BlobCacheStore for PendingRead {
    /// The fixture does not write disk bodies.
    fn persist(&self, _: u64, _: std::sync::Arc<[u8]>) {}

    /// The fixture does not expire disk entries.
    fn touch(&self, _: &[u64]) {}

    /// Only the selected hash waits indefinitely; other reads report no disk hit immediately.
    fn prefetch(&self, hashes: Vec<u64>) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            if hashes.contains(&self.0) {
                std::future::pending::<()>().await;
            }
        })
    }
}

/// Waiting for a later disk prefetch cannot disable recovery for admitted missing terrain.
#[test]
fn stalled_cached_reply_recovers_while_later_disk_prefetch_is_pending() {
    let hash = crate::client_blob_hash(b"missing terrain");
    let later_hash = crate::client_blob_hash(b"disk terrain");
    let (mut session, send, mut expires) = session(&[cached_reply(4, hash)], 1);
    let cache = ClientBlobCache::default().with_store(std::sync::Arc::new(PendingRead(later_hash)));
    session.blob_cache = Some(BlobCacheResolver::new(cache));
    assert!(poll_ingress(&mut session).is_ready());
    assert!(poll_ingress(&mut session).is_pending());
    send_packets(&send, &[cached_reply(5, later_hash)]);
    assert!(poll_ingress(&mut session).is_pending());
    expires.remove(0).send(()).unwrap();
    assert_recovery(&mut session, 4);
    assert!(
        session.pending_blob_prefetch.is_some(),
        "the later candidate survives cancellation"
    );
}
