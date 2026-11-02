//! End-to-end: a real relay on loopback UDP and three clients exercising proximity.

use client_voice::client::{ClientState, Incoming, VoiceClient};
use client_voice::codec::{FRAME_SAMPLES, FrameCodec, TestCodec};
use client_voice::proto::Position;
use client_voice::relay::{RelayConfig, run_udp};
use client_voice::room::RoomCode;
use std::net::{SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const SECRET: u64 = 0xFEED_FACE_CAFE_BEEF;

struct Room {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Room {
    fn start() -> Room {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = socket.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = thread::spawn(move || {
            run_udp(socket, RelayConfig::new(SECRET), &flag).unwrap();
        });
        Room {
            addr,
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for Room {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            h.join().unwrap();
        }
    }
}

/// Polls until `want` returns Some, or fails after two seconds.
fn wait_for<T>(client: &mut VoiceClient, mut want: impl FnMut(&Incoming) -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        for event in client.poll().unwrap() {
            if let Some(v) = want(&event) {
                return v;
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for event");
}

fn connect(room: &Room, id: u64, at: Position) -> VoiceClient {
    let mut c = VoiceClient::connect(room.addr, SECRET, id).unwrap();
    wait_for(&mut c, |e| {
        matches!(e, Incoming::Connected { .. }).then_some(())
    });
    assert_eq!(c.state(), ClientState::Connected);
    c.send_keepalive(at).unwrap();
    c
}

fn silence_for(client: &mut VoiceClient, ms: u64) -> Vec<Incoming> {
    thread::sleep(Duration::from_millis(ms));
    client.poll().unwrap()
}

#[test]
fn nearby_players_hear_each_other_and_distant_ones_do_not() {
    let room = Room::start();
    let here = Position::new(0, 0.0, 64.0, 0.0);
    let mut alice = connect(&room, 1, here);
    let mut bob = connect(&room, 2, Position::new(0, 8.0, 64.0, 0.0));
    let mut carol = connect(&room, 3, Position::new(0, 900.0, 64.0, 0.0));
    thread::sleep(Duration::from_millis(100)); // let keepalives land

    let mut codec = TestCodec::default();
    let pcm: Vec<i16> = (0..FRAME_SAMPLES)
        .map(|i| ((i % 100) as i16 - 50) * 200)
        .collect();
    let frame = codec.encode(&pcm).unwrap();
    alice.send_voice(here, &frame).unwrap();

    let (speaker, seq, payload) = wait_for(&mut bob, |e| match e {
        Incoming::Voice {
            speaker,
            seq,
            payload,
            ..
        } => Some((*speaker, *seq, payload.clone())),
        _ => None,
    });
    assert_eq!((speaker, seq), (1, 0));
    assert_eq!(payload, frame);
    let decoded = codec.decode(Some(&payload)).unwrap();
    assert_eq!(decoded.len(), FRAME_SAMPLES);

    assert!(
        silence_for(&mut carol, 150).is_empty(),
        "far player must hear nothing"
    );
    assert!(
        silence_for(&mut alice, 50).is_empty(),
        "speaker must not hear an echo"
    );
}

#[test]
fn wrong_secret_is_denied() {
    let room = Room::start();
    let mut intruder = VoiceClient::connect(room.addr, SECRET ^ 1, 9).unwrap();
    wait_for(&mut intruder, |e| {
        matches!(e, Incoming::Denied).then_some(())
    });
    assert_eq!(intruder.state(), ClientState::Denied);
}

#[test]
fn leaving_stops_delivery() {
    let room = Room::start();
    let here = Position::new(0, 0.0, 64.0, 0.0);
    let mut alice = connect(&room, 1, here);
    let bob = connect(&room, 2, here);
    thread::sleep(Duration::from_millis(100));
    drop(bob); // sends Bye
    thread::sleep(Duration::from_millis(100));
    alice.send_voice(here, &[1, 2, 3]).unwrap();
    assert!(silence_for(&mut alice, 100).is_empty());
}

#[test]
fn room_code_carries_everything_a_joiner_needs() {
    let room = Room::start();
    let SocketAddr::V4(v4) = room.addr else {
        unreachable!()
    };
    let code = RoomCode::new(SocketAddrV4::new(*v4.ip(), v4.port()), SECRET).encode();

    let parsed = RoomCode::decode(&code).unwrap();
    let mut joiner = VoiceClient::connect(parsed.socket_addr(), parsed.secret, 5).unwrap();
    wait_for(&mut joiner, |e| {
        matches!(e, Incoming::Connected { .. }).then_some(())
    });
    assert_eq!(joiner.range(), 48.0);
}
