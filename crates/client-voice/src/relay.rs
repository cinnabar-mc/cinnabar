//! The room host's relay: authenticates peers, tracks their positions and forwards each
//! voice frame only to listeners within range in the same dimension.
//!
//! [`Relay`] is a pure state machine (no sockets, no clock reads) so it is easy to test.
//! [`run_udp`] wraps it around a `UdpSocket`.

use crate::proto::{Packet, Position};
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct RelayConfig {
    pub secret: u64,
    /// Maximum hearing distance in blocks.
    pub range: f32,
    /// Peers silent for longer than this are dropped.
    pub timeout: Duration,
    pub max_peers: usize,
}

impl RelayConfig {
    pub fn new(secret: u64) -> Self {
        Self {
            secret,
            range: 48.0,
            timeout: Duration::from_secs(10),
            max_peers: 64,
        }
    }
}

#[derive(Debug)]
struct Peer {
    player_id: u64,
    pos: Option<Position>,
    last_seen: Instant,
}

/// A datagram the relay wants sent.
#[derive(Debug, Clone, PartialEq)]
pub struct Outgoing {
    pub to: SocketAddr,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct Relay {
    cfg: RelayConfig,
    peers: HashMap<SocketAddr, Peer>,
}

impl Relay {
    pub fn new(cfg: RelayConfig) -> Self {
        Self {
            cfg,
            peers: HashMap::new(),
        }
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    pub fn has_player(&self, player_id: u64) -> bool {
        self.peers.values().any(|p| p.player_id == player_id)
    }

    /// Handles one incoming datagram and returns the datagrams to send in response.
    pub fn handle(&mut self, from: SocketAddr, data: &[u8], now: Instant) -> Vec<Outgoing> {
        let Ok(packet) = Packet::decode(data) else {
            return Vec::new();
        };
        match packet {
            Packet::Hello { secret, player_id } => self.hello(from, secret, player_id, now),
            Packet::Voice { seq, pos, payload } => {
                let Some(speaker) = self.touch(from, Some(pos), now) else {
                    return Vec::new();
                };
                self.forward(from, speaker, seq, pos, payload)
            }
            Packet::KeepAlive { pos } => {
                self.touch(from, Some(pos), now);
                Vec::new()
            }
            Packet::Bye => {
                self.peers.remove(&from);
                Vec::new()
            }
            // Host-to-client packets are never valid input.
            Packet::Welcome { .. } | Packet::Denied | Packet::Relayed { .. } => Vec::new(),
        }
    }

    /// Drops peers that stopped sending. Returns their player ids.
    pub fn expire(&mut self, now: Instant) -> Vec<u64> {
        let timeout = self.cfg.timeout;
        let mut gone = Vec::new();
        self.peers.retain(|_, p| {
            let alive = now.saturating_duration_since(p.last_seen) <= timeout;
            if !alive {
                gone.push(p.player_id);
            }
            alive
        });
        gone
    }

    fn hello(
        &mut self,
        from: SocketAddr,
        secret: u64,
        player_id: u64,
        now: Instant,
    ) -> Vec<Outgoing> {
        let denied = || Outgoing {
            to: from,
            data: Packet::Denied.encode().unwrap(),
        };
        if secret != self.cfg.secret {
            return vec![denied()];
        }
        // A player reconnecting from a new port replaces its old session.
        self.peers
            .retain(|addr, p| *addr == from || p.player_id != player_id);
        if !self.peers.contains_key(&from) && self.peers.len() >= self.cfg.max_peers {
            return vec![denied()];
        }
        self.peers.insert(
            from,
            Peer {
                player_id,
                pos: None,
                last_seen: now,
            },
        );
        vec![Outgoing {
            to: from,
            data: Packet::Welcome {
                range: self.cfg.range,
            }
            .encode()
            .unwrap(),
        }]
    }

    /// Refreshes a known peer. Returns its player id, or `None` for unauthenticated senders.
    fn touch(&mut self, from: SocketAddr, pos: Option<Position>, now: Instant) -> Option<u64> {
        let peer = self.peers.get_mut(&from)?;
        peer.last_seen = now;
        if pos.is_some() {
            peer.pos = pos;
        }
        Some(peer.player_id)
    }

    fn forward(
        &self,
        from: SocketAddr,
        speaker: u64,
        seq: u32,
        pos: Position,
        payload: Vec<u8>,
    ) -> Vec<Outgoing> {
        let relayed = Packet::Relayed {
            speaker,
            seq,
            pos,
            payload,
        };
        let Ok(data) = relayed.encode() else {
            return Vec::new();
        };
        self.peers
            .iter()
            .filter(|(addr, _)| **addr != from)
            .filter(|(_, peer)| {
                peer.pos
                    .is_some_and(|p| p.dim == pos.dim && p.distance(&pos) <= self.cfg.range)
            })
            .map(|(addr, _)| Outgoing {
                to: *addr,
                data: data.clone(),
            })
            .collect()
    }
}

/// Runs the relay on `socket` until `stop` is set. Blocks the calling thread.
pub fn run_udp(socket: UdpSocket, cfg: RelayConfig, stop: &AtomicBool) -> io::Result<()> {
    socket.set_read_timeout(Some(Duration::from_millis(100)))?;
    let mut relay = Relay::new(cfg);
    let mut buf = [0u8; 2048];
    let mut last_expire = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                for out in relay.handle(from, &buf[..n], Instant::now()) {
                    let _ = socket.send_to(&out.data, out.to);
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(e),
        }
        let now = Instant::now();
        if now.duration_since(last_expire) >= Duration::from_secs(1) {
            relay.expire(now);
            last_expire = now;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: u64 = 42;

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn join(relay: &mut Relay, port: u16, id: u64, now: Instant) {
        let hello = Packet::Hello {
            secret: SECRET,
            player_id: id,
        }
        .encode()
        .unwrap();
        let out = relay.handle(addr(port), &hello, now);
        assert_eq!(
            Packet::decode(&out[0].data).unwrap(),
            Packet::Welcome { range: 48.0 }
        );
    }

    fn at(relay: &mut Relay, port: u16, p: Position, now: Instant) {
        relay.handle(
            addr(port),
            &Packet::KeepAlive { pos: p }.encode().unwrap(),
            now,
        );
    }

    fn speak(relay: &mut Relay, port: u16, p: Position, now: Instant) -> Vec<Outgoing> {
        let pkt = Packet::Voice {
            seq: 1,
            pos: p,
            payload: vec![1, 2, 3],
        };
        relay.handle(addr(port), &pkt.encode().unwrap(), now)
    }

    #[test]
    fn rejects_wrong_secret() {
        let mut relay = Relay::new(RelayConfig::new(SECRET));
        let hello = Packet::Hello {
            secret: 1,
            player_id: 1,
        }
        .encode()
        .unwrap();
        let out = relay.handle(addr(1), &hello, Instant::now());
        assert_eq!(Packet::decode(&out[0].data).unwrap(), Packet::Denied);
        assert_eq!(relay.peer_count(), 0);
    }

    #[test]
    fn ignores_voice_from_unknown_senders() {
        let mut relay = Relay::new(RelayConfig::new(SECRET));
        join(&mut relay, 1, 1, Instant::now());
        at(
            &mut relay,
            1,
            Position::new(0, 0.0, 0.0, 0.0),
            Instant::now(),
        );
        let out = speak(
            &mut relay,
            99,
            Position::new(0, 1.0, 0.0, 0.0),
            Instant::now(),
        );
        assert!(out.is_empty());
    }

    #[test]
    fn forwards_only_to_listeners_in_range_and_dimension() {
        let now = Instant::now();
        let mut relay = Relay::new(RelayConfig::new(SECRET));
        for (port, id) in [(1, 1), (2, 2), (3, 3), (4, 4)] {
            join(&mut relay, port, id, now);
        }
        at(&mut relay, 2, Position::new(0, 10.0, 64.0, 0.0), now); // near
        at(&mut relay, 3, Position::new(0, 500.0, 64.0, 0.0), now); // far
        at(&mut relay, 4, Position::new(1, 10.0, 64.0, 0.0), now); // other dimension
        let out = speak(&mut relay, 1, Position::new(0, 0.0, 64.0, 0.0), now);
        let targets: Vec<_> = out.iter().map(|o| o.to).collect();
        assert_eq!(targets, vec![addr(2)]);
        match Packet::decode(&out[0].data).unwrap() {
            Packet::Relayed { speaker, seq, .. } => assert_eq!((speaker, seq), (1, 1)),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn listeners_without_a_known_position_hear_nothing() {
        let now = Instant::now();
        let mut relay = Relay::new(RelayConfig::new(SECRET));
        join(&mut relay, 1, 1, now);
        join(&mut relay, 2, 2, now);
        assert!(speak(&mut relay, 1, Position::new(0, 0.0, 0.0, 0.0), now).is_empty());
    }

    #[test]
    fn expires_silent_peers() {
        let now = Instant::now();
        let mut relay = Relay::new(RelayConfig::new(SECRET));
        join(&mut relay, 1, 1, now);
        join(&mut relay, 2, 2, now);
        at(
            &mut relay,
            2,
            Position::new(0, 0.0, 0.0, 0.0),
            now + Duration::from_secs(9),
        );
        let gone = relay.expire(now + Duration::from_secs(11));
        assert_eq!(gone, vec![1]);
        assert!(relay.has_player(2) && !relay.has_player(1));
    }

    #[test]
    fn reconnecting_player_replaces_old_session() {
        let now = Instant::now();
        let mut relay = Relay::new(RelayConfig::new(SECRET));
        join(&mut relay, 1, 7, now);
        join(&mut relay, 5, 7, now);
        assert_eq!(relay.peer_count(), 1);
    }

    #[test]
    fn enforces_room_capacity() {
        let now = Instant::now();
        let mut cfg = RelayConfig::new(SECRET);
        cfg.max_peers = 1;
        let mut relay = Relay::new(cfg);
        join(&mut relay, 1, 1, now);
        let hello = Packet::Hello {
            secret: SECRET,
            player_id: 2,
        }
        .encode()
        .unwrap();
        let out = relay.handle(addr(2), &hello, now);
        assert_eq!(Packet::decode(&out[0].data).unwrap(), Packet::Denied);
    }

    #[test]
    fn bye_removes_the_peer_and_garbage_is_ignored() {
        let now = Instant::now();
        let mut relay = Relay::new(RelayConfig::new(SECRET));
        join(&mut relay, 1, 1, now);
        assert!(relay.handle(addr(1), &[1, 2, 3], now).is_empty());
        relay.handle(addr(1), &Packet::Bye.encode().unwrap(), now);
        assert_eq!(relay.peer_count(), 0);
    }
}
