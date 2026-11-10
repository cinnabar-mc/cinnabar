//! Server packets with IDs the protocol does not define are skipped without ending play.

use super::*;

/// An ID past the 1.26.60 packet table that a third-party server sent during a hub join.
const UNKNOWN_ID: u32 = 508;
const SENTINEL_TIME: i32 = 81_234;
const DRAIN_LIMIT: usize = 64;

#[tokio::test]
async fn unknown_packet_ids_are_counted_and_skipped_without_ending_play() {
    assert_eq!(PacketId::from_raw(UNKNOWN_ID), None);
    for cache_enabled in [false, true] {
        for world_receiver in [false, true] {
            let (mut session, core) = joined(Script {
                cache_enabled,
                ..Script::default()
            })
            .await;
            // Raw `recv` rejects the scripted out-of-bounds LevelChunk that world ingress skips,
            // so consume the scripted play traffic first.
            while !matches!(
                session.recv_world_event(0).await.unwrap(),
                WorldEvent::SetTime(protocol::SetTimeEvent {
                    time: SCRIPTED_PLAY_END_TIME
                })
            ) {}
            core.send_raw_id(UNKNOWN_ID, &[0x01, 0x02, 0x03]);
            core.send_batch(&[Packet::new(SetTime {
                time: wire::ZigZag32(SENTINEL_TIME),
            })]);
            let mut reached = false;
            for _ in 0..DRAIN_LIMIT {
                reached = if world_receiver {
                    matches!(
                        session.recv_world_event(0).await.unwrap(),
                        WorldEvent::SetTime(protocol::SetTimeEvent {
                            time: SENTINEL_TIME
                        })
                    )
                } else {
                    matches!(&*session.recv().await.unwrap().data,
                        packets::Packet::SetTime(packet) if packet.time.0 == SENTINEL_TIME)
                };
                if reached {
                    break;
                }
            }
            assert!(reached, "ordinary traffic follows an unknown packet ID");
            assert_eq!(session.unknown_packet_count(), 1);
            assert_eq!(session.decode_error_count(), 0);
            core.finish(session).await;
        }
    }
}
