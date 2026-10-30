//! A signed HTTP/DTLS/SCTP peer with generated Settings, Login and pack packets.

use bedrock_net::{
    BatchCodec, BatchHeader, BatchLimits, BatchPhase, CompressionAlgorithm, CompressionSettings,
    DecodeOutcome,
};
use bytes::Bytes;
use nethernet_tokio::{
    AcceptedSession, ClientIceConfig, ConnectionConfig, HttpServerConfig, HttpSignaling,
    HttpSignalingServer, NetherClient, NetherServer, SelfSignedIdentity, ServerIceConfig,
    ServerIdentity, TokenTrust,
};
use p384::{ecdsa::SigningKey, elliptic_curve::Generate};
use protocol::{
    Packet,
    test_support::negotiation::{self, ClientPacket},
};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::net::UdpSocket;

mod ingress;
mod metadata;

pub(super) struct Peer {
    pub(super) address: SocketAddr,
    signaling: Arc<HttpSignalingServer>,
    server: NetherServer,
    session: Option<AcceptedSession>,
}

enum Input {
    Message(Bytes),
    Closed,
}

impl Peer {
    /// Refuses the real Login before StartGame, then drives the original peer through retirement.
    pub(super) async fn refuse_login(mut self) {
        let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
        assert!(matches!(
            self.packet(&codec).await,
            ClientPacket::RequestNetworkSettings
        ));
        self.send(&codec, &[negotiation::network_settings(u16::MAX)])
            .await;
        let codec = codec.negotiate(CompressionSettings {
            algorithm: CompressionAlgorithm::Deflate,
            threshold: u16::MAX,
        });
        assert!(matches!(self.packet(&codec).await, ClientPacket::Login));
        self.send(&codec, &[negotiation::disconnect()]).await;
        assert!(
            matches!(self.next().await, Input::Closed),
            "refused Login produced another application message"
        );
        self.shutdown().await;
    }

    /// Binds actual HTTP signaling and reserves that same numeric tuple for server ICE.
    pub(super) async fn new() -> Self {
        let reservation = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = reservation.local_addr().unwrap();
        let signaling = Arc::new(
            HttpSignalingServer::bind(address, HttpServerConfig::default())
                .await
                .unwrap(),
        );
        let server = NetherServer::bind_with(
            Arc::clone(&signaling),
            ConnectionConfig {
                identity: Some(Arc::new(
                    ServerIdentity::generate("gateway-fixture.invalid", SystemTime::now()).unwrap(),
                )),
                server_ice: Some(ServerIceConfig {
                    turn_streams: Vec::new(),
                    turn_tls: None,
                    ice: Default::default(),
                    bind_addr: address,
                }),
                attempts: 1,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        drop(reservation);
        Self {
            address,
            signaling,
            server,
            session: None,
        }
    }

    /// Retires the same accepted session, listener and HTTP owners after client close.
    pub(super) async fn serve(mut self) {
        let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
        assert!(matches!(
            self.packet(&codec).await,
            ClientPacket::RequestNetworkSettings
        ));
        self.send(&codec, &[negotiation::network_settings(u16::MAX)])
            .await;
        let codec = codec.negotiate(CompressionSettings {
            algorithm: CompressionAlgorithm::Deflate,
            threshold: u16::MAX,
        });
        assert!(matches!(self.packet(&codec).await, ClientPacket::Login));
        self.send(&codec, &negotiation::login_success_and_pack_info())
            .await;
        assert!(matches!(
            self.packet(&codec).await,
            ClientPacket::ClientCacheStatus
        ));
        assert!(matches!(
            self.packet(&codec).await,
            ClientPacket::ResourcePackClientResponse
        ));
        self.send(&codec, &negotiation::empty_stack_and_start_game(42))
            .await;
        assert!(matches!(
            self.packet(&codec).await,
            ClientPacket::ResourcePackClientResponse
        ));
        // A bounded batch repeats valid text to exercise the real browser writer.
        let text =
            negotiation::raw_text("x".repeat(BatchLimits::default().max_decoded_bytes / 1024));
        let limits = BatchLimits::default();
        let encoded = negotiation::server_frame(&text);
        let count = (limits.max_decoded_bytes / 2 / encoded.len()).min(limits.max_packets);
        assert!(count > 1, "fixture must offer repeated legal text packets");
        let text = vec![text; count];
        for _ in 0..8 {
            self.send(&codec, &text).await;
        }
        match self.next().await {
            Input::Closed => {}
            Input::Message(_) => {
                panic!("empty-upload browser submitted an unexpected application message")
            }
        }
        self.shutdown().await;
    }

    /// Decodes the original client batch using the same public codec and generated decoder.
    async fn packet<S: BatchPhase>(&mut self, codec: &BatchCodec<S>) -> ClientPacket {
        negotiation::client_packet(&self.entry(codec).await)
    }

    /// Preserves one actual length-free entry for generated decoding and signature verification.
    async fn entry<S: BatchPhase>(&mut self, codec: &BatchCodec<S>) -> Bytes {
        let Input::Message(message) = self.next().await else {
            panic!("peer retired before the native negotiation completed")
        };
        let DecodeOutcome::Packets { mut packets, .. } = codec.decode(message).unwrap() else {
            panic!("expected an actual client batch")
        };
        assert_eq!(packets.len(), 1);
        packets.next().unwrap()
    }

    /// Frames generated server packets and submits them through the actual established core.
    async fn send<S: BatchPhase>(&mut self, codec: &BatchCodec<S>, packets: &[Packet]) {
        let limits = BatchLimits::default();
        let entries =
            negotiation::server_entries(packets, limits.max_packets, limits.max_decoded_bytes);
        let messages = codec.encode(&entries).unwrap();
        assert_eq!(messages.len(), 1, "fixture input must fit one real batch");
        for message in messages {
            self.session
                .as_ref()
                .expect("actual established peer")
                .session
                .send(message)
                .await
                .unwrap();
        }
    }

    /// Receives the same genuine accepted association without another application FIFO.
    async fn next(&mut self) -> Input {
        if self.session.is_none() {
            self.session = Some(self.server.accept().await.unwrap());
        }
        match self.session.as_mut().unwrap().reliable.recv().await {
            Ok(Some(message)) => Input::Message(message),
            Ok(None) | Err(_) => Input::Closed,
        }
    }

    /// Joins the actor and releases both original network tuples before returning.
    async fn shutdown(mut self) {
        if let Some(session) = self.session.take() {
            let session = session.close();
            session.session.closed().await;
            drop(session);
        }
        self.server.close().await.unwrap();
        drop(self.server);
        Arc::try_unwrap(self.signaling)
            .ok()
            .expect("original HTTP signaling retired")
            .shutdown()
            .await;
    }
}

/// Creates one actual signed association and retains the exact proof used by Login.
pub(super) async fn connect(address: SocketAddr) -> (NetherClient, Arc<SigningKey>, SocketAddr) {
    let reservation = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let local = reservation.local_addr().unwrap();
    let key = Arc::new(SigningKey::generate());
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let http = crate::upstream::http_client(&address.ip().to_string(), address, deadline).unwrap();
    let signaling = HttpSignaling::with_client("gateway-fixture-client".into(), http);
    let config = ConnectionConfig {
        identity: Some(Arc::new(
            SelfSignedIdentity::from_key(
                Arc::clone(&key),
                SystemTime::now(),
                Duration::from_secs(30),
            )
            .unwrap(),
        )),
        server_trust: Some(TokenTrust::Any),
        client_ice: Some(ClientIceConfig {
            turn_streams: Vec::new(),
            turn_tls: None,
            ice: Default::default(),
            bind_addr: Some(local),
        }),
        attempts: 1,
        ..Default::default()
    };
    drop(reservation);
    let connection = NetherClient::connect_with(signaling, format!("http://{address}"), config)
        .await
        .unwrap();
    (connection, key, local)
}
