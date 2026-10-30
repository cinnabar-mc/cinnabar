//! Pending archive I/O preserves complete packets from the original NetherNet session.

use super::*;
use crate::login::native_network::transport::{
    connect,
    early_spawn_tests::{FixtureRoute, Route, fixture_route},
};
use bedrock_client::packs::{PackOffer, PackTransferLimits};
use bedrock_client_tokio::PacketIo;
use bedrock_net::{BatchCodec, BatchHeader, BatchLimits};
use bedrock_protocol_contract_1_26_60::{packets, wire};
use resource_pack::test_support::archive_bytes;
use std::time::Duration;
use tokio::sync::oneshot;

#[tokio::test]
async fn pending_pack_cache_io_preserves_nethernet_packet_prefix() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let root = tempfile::tempdir().unwrap();
        let cache = open_cache(Some(root.path())).await.unwrap();
        let cancelled = Cancellation::new();
        let id = uuid::Uuid::from_u128(6);
        let archive = archive_bytes(id, &[]);
        let mut offered = PackInfoData::default();
        offered.pack_id_version.pack_uuid = id;
        offered.pack_id_version.pack_version.version = "1.0.0".into();
        offered.pack_size = wire::U64LE(archive.len() as u64);
        let mut offer = PackOffer::new(
            packets::ResourcePacksInfo {
                resource_packs: vec![offered.clone()],
                ..Default::default()
            },
            PackTransferLimits::default(),
        )
        .unwrap();
        offer.supply_preloaded(id, archive.clone()).unwrap();
        let mut stores = Stores::new();
        let held = Arc::clone(&stores.slot).acquire_owned().await.unwrap();
        stores.register(&offered);
        let (offer, _) = offer.start().unwrap();
        stores.queue_completed(
            &Some(Arc::clone(&cache)),
            |id| offer.completed_archive(id),
            &cancelled.0,
        );
        assert_eq!(stores.jobs.len(), 1);
        let expected = [1, 2].map(|time| {
            crate::Packet::new(packets::SetTime {
                time: wire::ZigZag32(time),
            })
        });
        let entries = expected
            .iter()
            .map(crate::session_transport::tests::packet_bytes)
            .collect::<Vec<_>>();
        let mut batches = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default())
            .encode(&entries)
            .unwrap();
        assert_eq!(batches.len(), 1);
        let FixtureRoute {
            mut listener,
            target,
            ..
        } = fixture_route(Route::Lan).await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        let (client, peer) =
            tokio::join!(connect(&target, None, None, deadline), listener.accept());
        let peer = peer.unwrap();
        let mut connection = crate::login::native_network::connection::upstream(client.unwrap());
        let (ready, waiting) = oneshot::channel();
        let serving = async {
            waiting.await.unwrap();
            peer.session.send(batches.remove(0)).await.unwrap();
            drop(held);
            peer.session.closed().await;
        };
        let application = async {
            let mut completing = Box::pin(super::super::wait_io(&mut connection, stores.finish()));
            assert!(futures::poll!(completing.as_mut()).is_pending());
            ready.send(()).unwrap();
            completing.await;
            for packet in &expected {
                let actual = crate::login::native_network::connection::next_packet(&mut connection)
                    .await
                    .unwrap();
                assert_eq!(&actual, packet);
            }
            connection.close().await.unwrap();
        };
        tokio::join!(serving, application);
        listener.close().await.unwrap();
        let loaded = preload(&offered, Some(cache), None, Arc::clone(&cancelled.0)).await;
        assert!(loaded.cache_hit);
        assert_eq!(loaded.archive, Some(archive));
    })
    .await
    .expect("bounded NetherNet cache fixture stalled");
}
