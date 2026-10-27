use super::*;
use bedrock_protocol_1_26_51::enums::PlayerPositionModeComponentPositionMode;
use bedrock_protocol_1_26_51::packets::{MoveActorAbsolute, MovePlayer};
use bedrock_protocol_1_26_51::types::{MoveActorAbsoluteData, MovePlayerTeleportData};

const ACTORS: u64 = 8;
const SEEK_TICKS: u32 = 300;
const DESTINATION: [f32; 3] = [120.0, 80.0, -40.0];

/// A rewind flush can contain every actor's movement across many recorded ticks.
fn rewind_burst() -> Vec<Packet> {
    let mut packets = Vec::new();
    for tick in (0..SEEK_TICKS).rev() {
        for actor in 0..ACTORS {
            packets.push(Packet::new(MoveActorAbsolute {
                move_data: MoveActorAbsoluteData {
                    actor_runtime_id: ActorRuntimeID(OTHER_RUNTIME_ID + actor),
                    position: [tick as f32, 70.0, actor as f32].into(),
                    ..Default::default()
                },
            }));
        }
    }
    packets.push(Packet::new(MovePlayer {
        player_runtime_id: ActorRuntimeID(RUNTIME_ID),
        position: DESTINATION.into(),
        position_mode: PlayerPositionModeComponentPositionMode::Teleport,
        teleport_data: Some(MovePlayerTeleportData::default()),
        ..Default::default()
    }));
    packets
}

#[test]
fn raw_codec_accepts_a_replay_burst() {
    let frame = batch(&rewind_burst());
    let mut bytes = frame.to_vec();
    bytes[0] = wire::BATCH_HEADER;
    let packets = protocol::decode_batch(bytes.into()).expect("valid server batch");
    assert_eq!(packets.len(), ACTORS as usize * SEEK_TICKS as usize + 1);
    assert_eq!(packets.last().unwrap().header.id, PacketId::MovePlayer);
}

#[tokio::test]
async fn replay_bursts_preserve_movement_teleports_and_the_live_session() {
    for cache_enabled in [false, true] {
        let (mut session, _, core) = join(Script {
            cache_enabled,
            ..Default::default()
        })
        .await
        .unwrap();
        tokio::time::timeout(CORE_TIMEOUT, async {
            loop {
                match session.recv_world_event(0).await.expect("scripted prelude") {
                    WorldEvent::NetworkStackLatency(timestamp) => session
                        .send(protocol::network_stack_latency_reply(timestamp))
                        .await
                        .unwrap(),
                    WorldEvent::SetTime(time) if time.time == SCRIPTED_PLAY_END_TIME => break,
                    _ => {}
                }
            }
            let decode_errors = session.decode_error_count();
            for _ in 0..2 {
                core.send_batch(&rewind_burst());
                for tick in (0..SEEK_TICKS).rev() {
                    for actor in 0..ACTORS {
                        let event = session.recv_world_event(0).await.expect("replay movement");
                        let WorldEvent::Actor(protocol::ActorEvent::Move(movement)) = event else {
                            panic!("expected movement, got {event:?}");
                        };
                        assert_eq!(movement.runtime_id, OTHER_RUNTIME_ID + actor);
                        assert_eq!(
                            movement.position,
                            [Some(tick as f32), Some(70.0), Some(actor as f32)]
                        );
                    }
                }
                let event = session.recv_world_event(0).await.expect("viewer teleport");
                let WorldEvent::MovePlayer(movement) = event else {
                    panic!("expected viewer teleport, got {event:?}");
                };
                assert_eq!(movement.runtime_id, RUNTIME_ID);
                assert_eq!(movement.position, DESTINATION);
                assert!(movement.teleported);
            }
            core.send_batch(&[set_time(SCRIPTED_PLAY_END_TIME)]);
            assert_eq!(
                session
                    .recv_world_event(0)
                    .await
                    .expect("session stays open"),
                WorldEvent::SetTime(protocol::SetTimeEvent {
                    time: SCRIPTED_PLAY_END_TIME,
                })
            );
            assert_eq!(session.decode_error_count(), decode_errors);
        })
        .await
        .expect("replay traffic must keep progressing");
        core.finish(session).await;
    }
}
