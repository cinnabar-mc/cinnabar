//! A stalled application receiver keeps NetherNet writes live and retains the accepted FIFO.

use super::{
    connect,
    early_spawn_tests::{FixtureRoute, Route, fixture_route, receive},
};
use crate::session_transport::{batch_body, native};
use bedrock_net::{BatchCodec, BatchHeader, BatchLimits};
use bedrock_protocol_contract_1_26_60::{packets, wire};
use bytes::Bytes;
use std::time::Duration;
use tokio::sync::oneshot;

/// Uses the same packet in its transport batch and in the application pump body.
fn message(index: i32) -> (Bytes, Bytes) {
    let packet = crate::Packet::new(packets::SetTime {
        time: wire::ZigZag32(index),
    });
    let entry = crate::session_transport::tests::packet_bytes(&packet);
    let batch = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default())
        .encode(&[entry])
        .unwrap()
        .remove(0);
    (batch, batch_body(&[packet]).unwrap())
}

#[tokio::test]
async fn stalled_native_consumer_keeps_nethernet_writes_and_accepted_prefix() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let FixtureRoute {
            mut listener,
            target,
            ..
        } = fixture_route(Route::Lan).await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        let (client, peer) =
            tokio::join!(connect(&target, None, None, deadline), listener.accept());
        let mut peer = peer.unwrap();
        let connection = crate::login::native_network::connection::upstream(client.unwrap());
        let total = BatchLimits::default().max_packets.checked_add(2).unwrap();
        let first_body = message(0).1;
        let budget = first_body.len().checked_sub(1).unwrap();
        assert_ne!(budget, 0);
        let native::PreparedPump {
            sender,
            mut incoming,
            stop,
            driver,
        } = native::prepare_with_byte_budget(connection, Some(budget));
        let (pressure, pressured) = oneshot::channel();
        let (marker_seen, seen) = oneshot::channel();
        let serving = async {
            peer.session.send(message(0).0).await.unwrap();
            pressured.await.unwrap();
            let outbound = peer.session.sender();
            let sending = async {
                for index in 1..total {
                    outbound
                        .send(message(index.try_into().unwrap()).0)
                        .await
                        .unwrap();
                }
            };
            let reading = async {
                let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
                let marker = receive(&mut peer, &codec).await;
                assert_eq!(
                    marker,
                    [crate::Packet::new(packets::SetTime {
                        time: wire::ZigZag32(-1)
                    })]
                );
                marker_seen.send(()).unwrap();
            };
            tokio::join!(sending, reading);
            peer.session.closed().await;
        };
        let application = async {
            let consuming = async {
                let first = incoming.recv().await.unwrap().into_body().unwrap();
                assert_eq!(first, first_body);
                pressure.send(()).unwrap();
                let written = sender.accept(message(-1).1).await.unwrap();
                written.await.unwrap().unwrap();
                seen.await.unwrap();
                drop(first);
                for index in 1..total {
                    let actual = incoming.recv().await.unwrap().into_body().unwrap();
                    assert_eq!(actual, message(index.try_into().unwrap()).1);
                }
                let _ = stop.send(());
                drop(sender);
                drop(incoming);
            };
            let (end, ()) = tokio::join!(driver, consuming);
            end.close.unwrap();
            drop(end.pending);
        };
        tokio::join!(serving, application);
        listener.close().await.unwrap();
    })
    .await
    .expect("bounded NetherNet pressure fixture stalled");
}
