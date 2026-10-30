//! Signed Login metadata retains the selected logical origin independently of its physical socket.

use super::*;
use bedrock_net::login::{client_data::RawClientData, parse_offline_request};
use protocol::NativeRelayConnecting;

/// Default ports must remain explicit in the real signed Login, including a selected fallback.
#[tokio::test]
async fn signed_login_keeps_logical_http_host_and_effective_port() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let fallback = protocol::native_http_endpoints("fallback.invalid").unwrap();
        let cases = [
            ("https://play.invalid", "https://play.invalid:443"),
            ("http://play.invalid", "http://play.invalid:80"),
            ("https://[2001:db8::1]", "https://[2001:db8::1]:443"),
            ("http://[2001:db8::1]", "http://[2001:db8::1]:80"),
            (fallback.last().unwrap(), "http://fallback.invalid:80"),
        ];
        for (selected, expected) in cases {
            let mut peer = Peer::new().await;
            let address = peer.address;
            let (proof, offered) = tokio::sync::oneshot::channel();
            let server = async {
                let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
                assert_eq!(
                    peer.packet(&codec).await,
                    ClientPacket::RequestNetworkSettings
                );
                peer.send(&codec, &[negotiation::network_settings(u16::MAX)])
                    .await;
                let codec = codec.negotiate(CompressionSettings {
                    algorithm: CompressionAlgorithm::Deflate,
                    threshold: u16::MAX,
                });
                let entry = peer.entry(&codec).await;
                let request = negotiation::client_login_request(&entry);
                let verified = parse_offline_request(
                    &request,
                    SystemTime::now(),
                    BatchLimits::default().max_decoded_bytes,
                    |_| panic!("HTTP Login metadata must not authorize or resolve a socket"),
                )
                .unwrap();
                let claims: serde_json::Value =
                    serde_json::from_str(verified.client_data().get()).unwrap();
                assert_eq!(claims["ServerAddress"], expected);
                assert_eq!(
                    verified.signing_key().as_p384(),
                    Some(&offered.await.unwrap())
                );
                peer.send(&codec, &[negotiation::disconnect()]).await;
                assert!(matches!(peer.next().await, Input::Closed));
                peer.shutdown().await;
            };
            let client = async {
                // Real loopback HTTP/DTLS/SCTP supplies I/O; the selected logical URL supplies metadata only.
                let (actual, key, _) = super::connect(address).await;
                proof.send(*key.verifying_key()).unwrap();
                let label = protocol::native_http_login_address(selected).unwrap();
                let mut joining = NativeRelayConnecting::from_http(actual, label, key);
                let random = protocol::ClientRandomId::new(1).unwrap();
                let mut claims = protocol::login_client_data(
                    "Fixture",
                    random,
                    None,
                    &protocol::LoginSettings::default(),
                );
                protocol::NativeDeviceProfile::fresh()
                    .apply(&mut claims, random, "Fixture")
                    .unwrap();
                let claims = serde_json::value::to_raw_value(&claims).unwrap();
                assert!(
                    joining
                        .negotiate(RawClientData::from_raw(&claims).unwrap(), false, None)
                        .await
                        .is_err()
                );
                let mut retiring = joining.begin_close();
                retiring.finish_close().await.unwrap();
            };
            tokio::join!(server, client);
        }
    })
    .await
    .expect("real signed Login metadata fixture stalled");
}
