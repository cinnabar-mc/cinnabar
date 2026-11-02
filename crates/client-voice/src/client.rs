//! Non-blocking UDP client for a voice room. The game thread calls [`VoiceClient::poll`]
//! each frame and [`VoiceClient::send_voice`] for every transmitted audio frame.

use crate::proto::{Packet, Position};
use std::io;
use std::net::{SocketAddr, UdpSocket};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientState {
    Connecting,
    Connected,
    Denied,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Connected {
        range: f32,
    },
    Denied,
    Voice {
        speaker: u64,
        seq: u32,
        pos: Position,
        payload: Vec<u8>,
    },
}

#[derive(Debug)]
pub struct VoiceClient {
    socket: UdpSocket,
    host: SocketAddr,
    secret: u64,
    player_id: u64,
    seq: u32,
    state: ClientState,
    range: f32,
}

impl VoiceClient {
    /// Opens a socket toward `host` and sends the first `Hello`.
    pub fn connect(host: SocketAddr, secret: u64, player_id: u64) -> io::Result<Self> {
        let bind: SocketAddr = if host.is_ipv4() {
            ([0, 0, 0, 0], 0).into()
        } else {
            ([0u16; 8], 0).into()
        };
        let socket = UdpSocket::bind(bind)?;
        socket.set_nonblocking(true)?;
        let client = Self {
            socket,
            host,
            secret,
            player_id,
            seq: 0,
            state: ClientState::Connecting,
            range: 0.0,
        };
        client.send_hello()?;
        Ok(client)
    }

    pub fn state(&self) -> ClientState {
        self.state
    }

    /// Maximum hearing distance announced by the host (0 until connected).
    pub fn range(&self) -> f32 {
        self.range
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// Resend while `Connecting`: UDP datagrams can be lost.
    pub fn send_hello(&self) -> io::Result<()> {
        self.send(&Packet::Hello {
            secret: self.secret,
            player_id: self.player_id,
        })
    }

    /// Sends one encoded frame. The sequence number only advances for frames actually sent.
    pub fn send_voice(&mut self, pos: Position, payload: &[u8]) -> io::Result<()> {
        let packet = Packet::Voice {
            seq: self.seq,
            pos,
            payload: payload.to_vec(),
        };
        self.send(&packet)?;
        self.seq = self.seq.wrapping_add(1);
        Ok(())
    }

    /// Sends the current position while not talking; call about once a second.
    pub fn send_keepalive(&self, pos: Position) -> io::Result<()> {
        self.send(&Packet::KeepAlive { pos })
    }

    pub fn leave(&self) {
        let _ = self.send(&Packet::Bye);
    }

    /// Drains every datagram that has arrived. Never blocks.
    pub fn poll(&mut self) -> io::Result<Vec<Incoming>> {
        let mut events = Vec::new();
        let mut buf = [0u8; 2048];
        loop {
            match self.socket.recv_from(&mut buf) {
                Ok((n, from)) => {
                    if from != self.host {
                        continue; // only the host may talk to us
                    }
                    match Packet::decode(&buf[..n]) {
                        Ok(Packet::Welcome { range }) => {
                            self.state = ClientState::Connected;
                            self.range = range;
                            events.push(Incoming::Connected { range });
                        }
                        Ok(Packet::Denied) => {
                            self.state = ClientState::Denied;
                            events.push(Incoming::Denied);
                        }
                        Ok(Packet::Relayed {
                            speaker,
                            seq,
                            pos,
                            payload,
                        }) => {
                            events.push(Incoming::Voice {
                                speaker,
                                seq,
                                pos,
                                payload,
                            });
                        }
                        _ => {}
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(events),
                // Windows reports ICMP "port unreachable" from a previous send as a reset.
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => continue,
                Err(e) => return Err(e),
            }
        }
    }

    fn send(&self, packet: &Packet) -> io::Result<()> {
        let data = packet
            .encode()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        self.socket.send_to(&data, self.host).map(|_| ())
    }
}

impl Drop for VoiceClient {
    fn drop(&mut self) {
        self.leave();
    }
}
