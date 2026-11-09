use super::*;
use crate::Packet;
use bedrock_protocol_1_26_60::packets::{self, ClientCacheMissResponse, SetTime};
use bedrock_protocol_1_26_60::types::MissingBlobData;
use bedrock_protocol_1_26_60::wire;
use bytes::Bytes;

/// Constructs one body reply without involving cache admission or transport writes.
fn body_response() -> Packet {
    Packet::new(ClientCacheMissResponse {
        missing_blobs: vec![MissingBlobData {
            blob_id: wire::U64LE(crate::client_blob_hash(b"body")),
            blob_data: Bytes::from_static(b"body"),
        }],
    })
}

/// Constructs ordinary input with an observable FIFO position.
fn ordinary(time: i32) -> Packet {
    Packet::new(SetTime {
        time: wire::ZigZag32(time),
    })
}

/// Constructs terrain metadata that can confirm an armed backend boundary.
fn terrain() -> Packet {
    Packet::new(packets::LevelChunk {
        cache_enabled: true,
        cache_metadata: vec![bedrock_protocol_1_26_60::types::SubChunkMetadata(1)],
        ..Default::default()
    })
}

/// Preloads a bounded synthetic batch; tests never need to poll the empty reader.
fn transport(packets: &[Packet]) -> SessionTransport<bridge::FramedReader<tokio::io::Empty>> {
    let (reader, frames, _writer) = bridge::tasks::prepare_connection(
        tokio::io::empty(),
        tokio::io::sink(),
        bridge::MAX_FRAME_LEN,
    );
    let startup: Vec<_> = packets
        .iter()
        .map(crate::session_transport::tests::packet_bytes)
        .collect();
    SessionTransport::new(reader, frames, &startup)
}

/// Extracted bodies leave both earlier and later ordinary packets in their original order.
#[tokio::test]
async fn blob_intake_preserves_raw_fifo_across_multiple_bodies() {
    let mut transport = transport(&[
        ordinary(1),
        body_response(),
        ordinary(2),
        body_response(),
        ordinary(3),
    ]);
    for _ in 0..2 {
        assert!(
            matches!(transport.recv_blob_body(1024, false).await.unwrap(), Some(Inbound::Packet(raw)) if raw.id == PacketId::ClientCacheMissResponse)
        );
    }
    for time in 1..=3 {
        let Inbound::Packet(raw) = transport.recv().await.unwrap() else {
            panic!("ordinary packet")
        };
        assert!(
            matches!(*raw.decode().unwrap().data, packets::Packet::SetTime(packet) if packet.time.0 == time)
        );
    }
}

/// A raw-memory frontier returns ownership to the caller without dropping the inspected prefix.
#[tokio::test]
async fn blob_intake_respects_raw_byte_capacity() {
    let mut transport = transport(&[ordinary(1)]);
    assert!(transport.recv_blob_body(1, false).await.unwrap().is_none());
    let Inbound::Packet(raw) = transport.recv().await.unwrap() else {
        panic!("retained ordinary packet")
    };
    assert_eq!(raw.id, PacketId::SetTime);
}

/// A body's bytes after a world or backend boundary must never resolve the previous cache epoch.
#[tokio::test]
async fn blob_intake_stops_before_dimension_and_fast_transfer_boundaries() {
    for (boundary_id, boundary, transfer_armed) in [
        (
            PacketId::ChangeDimension,
            Packet::new(packets::ChangeDimension::default()),
            false,
        ),
        (
            PacketId::Transfer,
            Packet::new(packets::Transfer::default()),
            false,
        ),
        (
            PacketId::Disconnect,
            Packet::new(packets::Disconnect::default()),
            false,
        ),
        (PacketId::LevelChunk, terrain(), true),
        (
            PacketId::SubChunk,
            Packet::new(packets::SubChunk::default()),
            true,
        ),
    ] {
        let mut transport = transport(&[ordinary(1), boundary, body_response()]);
        assert!(
            transport
                .recv_blob_body(1024, transfer_armed)
                .await
                .unwrap()
                .is_none()
        );
        for id in [
            PacketId::SetTime,
            boundary_id,
            PacketId::ClientCacheMissResponse,
        ] {
            assert!(
                matches!(transport.recv().await.unwrap(), Inbound::Packet(raw) if raw.id == id)
            );
        }
    }
}

/// Unarmed terrain metadata is retained while bodies for earlier work pass it safely.
#[tokio::test]
async fn blob_intake_retains_later_terrain_before_its_ordinary_decode() {
    let later = terrain();
    let mut transport = transport(&[ordinary(1), later, body_response()]);
    assert!(
        matches!(transport.recv_blob_body(1024, false).await.unwrap(), Some(Inbound::Packet(raw)) if raw.id == PacketId::ClientCacheMissResponse)
    );
    assert!(
        matches!(transport.recv().await.unwrap(), Inbound::Packet(raw) if raw.id == PacketId::SetTime)
    );
    assert!(
        matches!(transport.recv().await.unwrap(), Inbound::Packet(raw) if raw.id == PacketId::LevelChunk)
    );
}

/// A core terminal remains behind retained raw packets even when body look-ahead reads it.
#[cfg(unix)]
#[tokio::test]
async fn blob_intake_retains_core_terminal_in_fifo_order() {
    use crate::session_transport::tests::unix::{pair, write_frame};
    let startup = [crate::session_transport::tests::packet_bytes(&ordinary(1))];
    let mut pair = pair("blob-terminal", &startup).await;
    let mut terminal = vec![bridge::contract::KIND_DISCONNECT];
    terminal.extend_from_slice(
        br#"{"reason":0,"message":"fixture","filtered_message":"","hide_screen":false}"#,
    );
    write_frame(&mut pair.peer, &terminal).await;
    assert!(
        pair.transport
            .recv_blob_body(1024, false)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        matches!(pair.transport.recv().await.unwrap(), Inbound::Packet(raw) if raw.id == PacketId::SetTime)
    );
    assert!(matches!(
        pair.transport.recv().await.unwrap(),
        Inbound::Terminal(super::super::Terminal::Disconnect(_))
    ));
}

/// Arming a transfer after cancellation rechecks the retained prefix before accepting a body.
#[cfg(unix)]
#[tokio::test]
async fn blob_intake_rechecks_transfer_boundary_after_cancelled_read() {
    use crate::session_transport::tests::unix::pair;
    use std::future::Future;
    use std::task::{Context, Poll};
    let startup = [ordinary(1), terrain()]
        .iter()
        .map(crate::session_transport::tests::packet_bytes)
        .collect::<Vec<_>>();
    let mut pair = pair("blob-transfer-arm", &startup).await;
    {
        let mut receive = std::pin::pin!(pair.transport.recv_blob_body(1024, false));
        assert!(matches!(
            receive
                .as_mut()
                .poll(&mut Context::from_waker(std::task::Waker::noop())),
            Poll::Pending
        ));
    }
    assert!(
        pair.transport
            .recv_blob_body(1024, true)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        matches!(pair.transport.recv().await.unwrap(), Inbound::Packet(raw) if raw.id == PacketId::SetTime)
    );
    assert!(
        matches!(pair.transport.recv().await.unwrap(), Inbound::Packet(raw) if raw.id == PacketId::LevelChunk)
    );
}

/// Raw packets and previously read frames share the same byte frontier before another frame splits.
#[cfg(unix)]
#[tokio::test]
async fn blob_intake_shares_capacity_with_unparsed_read_ahead() {
    use crate::session_transport::{
        batch_frame,
        tests::unix::{pair, write_frame},
    };
    let startup = [crate::session_transport::tests::packet_bytes(&ordinary(1))];
    let mut pair = pair("blob-shared-bytes", &startup).await;
    let frame = batch_frame(&[ordinary(2), body_response()]).unwrap();
    write_frame(&mut pair.peer, &frame).await;
    let budget = frame.len() + 1;
    assert!(std::future::poll_fn(|cx| pair.transport.poll_read_ahead_one(budget, cx)).await);
    assert!(
        pair.transport
            .recv_blob_body(budget, false)
            .await
            .unwrap()
            .is_none()
    );
    for id in [
        PacketId::SetTime,
        PacketId::SetTime,
        PacketId::ClientCacheMissResponse,
    ] {
        assert!(
            matches!(pair.transport.recv().await.unwrap(), Inbound::Packet(raw) if raw.id == id)
        );
    }
}
