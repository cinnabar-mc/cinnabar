//! Two real sessions over loopback UDP with fake audio devices: Alice hosts and talks,
//! Bob joins with the room code and must hear her, positioned in stereo.

use cinnabar_voice::engine::{AudioIo, Status, VoiceHandle};
use cinnabar_voice::proto::Position;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Produces a steady tone as "microphone" input and records what was played.
#[derive(Clone)]
struct FakeIo {
    tone: bool,
    sample_clock: Arc<Mutex<usize>>,
    last_read: Arc<Mutex<Instant>>,
    played: Arc<Mutex<Vec<i16>>>,
}

impl FakeIo {
    fn new(tone: bool) -> Self {
        Self {
            tone,
            sample_clock: Arc::new(Mutex::new(0)),
            last_read: Arc::new(Mutex::new(Instant::now())),
            played: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Mean absolute level of the left and right channels played so far.
    fn levels(&self) -> (f32, f32) {
        let played = self.played.lock().unwrap();
        let (mut l, mut r, mut n) = (0f64, 0f64, 0f64);
        for pair in played.chunks_exact(2) {
            l += f64::from(pair[0]).abs();
            r += f64::from(pair[1]).abs();
            n += 1.0;
        }
        if n == 0.0 {
            (0.0, 0.0)
        } else {
            ((l / n) as f32, (r / n) as f32)
        }
    }
}

impl AudioIo for FakeIo {
    fn read_mic(&mut self, out: &mut Vec<i16>) {
        let mut last = self.last_read.lock().unwrap();
        let now = Instant::now();
        let elapsed = now.duration_since(*last);
        *last = now;
        let samples = (elapsed.as_secs_f64() * 48_000.0) as usize;
        let mut clock = self.sample_clock.lock().unwrap();
        for _ in 0..samples {
            let value = if self.tone {
                let t = *clock as f32 / 48_000.0;
                (9000.0 * (2.0 * std::f32::consts::PI * 300.0 * t).sin()) as i16
            } else {
                0
            };
            out.push(value);
            *clock += 1;
        }
    }

    fn write_playback(&mut self, stereo: &[i16]) {
        self.played.lock().unwrap().extend_from_slice(stereo);
    }
}

fn wait_until(what: &str, cond: impl FnMut() -> bool) {
    wait_until_for(what, Duration::from_secs(5), cond);
}

fn wait_until_for(what: &str, limit: Duration, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for: {what}");
}

fn start_pair(
    alice_pos: Position,
    bob_pos: Position,
) -> (VoiceHandle, VoiceHandle, FakeIo, FakeIo) {
    let alice_io = FakeIo::new(true);
    let bob_io = FakeIo::new(false);
    let alice = VoiceHandle::spawn(alice_io.clone(), 1001);
    let bob = VoiceHandle::spawn(bob_io.clone(), 2002);
    alice.set_local_player(alice_pos, 0.0);
    bob.set_local_player(bob_pos, 0.0);

    // Port 0 would be nicer, but the session always uses the default room port; tests in
    // this file run one after another because they share it.
    alice.host_room(Some("127.0.0.1".parse().unwrap()));
    wait_until("alice hosts", || alice.snapshot().room_code.is_some());
    let code = alice.snapshot().room_code.unwrap();
    wait_until("alice connected", || {
        alice.snapshot().status == Status::Connected
    });

    bob.join_room(&code).unwrap();
    wait_until("bob connected", || {
        bob.snapshot().status == Status::Connected
    });
    (alice, bob, alice_io, bob_io)
}

static SERIAL: Mutex<()> = Mutex::new(());

#[test]
fn bob_hears_alice_from_her_side_of_the_stereo_field() {
    let _guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // Bob faces south (yaw 0); Alice stands west of him, so she must be on his right.
    let (alice, bob, _alice_io, bob_io) = start_pair(
        Position::new(0, -6.0, 64.0, 0.0),
        Position::new(0, 0.0, 64.0, 0.0),
    );

    wait_until("bob hears audio", || {
        let (l, r) = bob_io.levels();
        l + r > 200.0
    });
    thread::sleep(Duration::from_millis(300));
    let (left, right) = bob_io.levels();
    assert!(
        right > left * 4.0,
        "alice should be on bob's right: left={left} right={right}"
    );

    let speakers = bob.snapshot().speakers;
    assert_eq!(speakers.len(), 1);
    assert_eq!(speakers[0].id, 1001);
    assert!(speakers[0].level > 0.01);
    assert!(alice.snapshot().transmitting || alice.snapshot().mic_level > 0.0);

    alice.shutdown();
    bob.shutdown();
}

#[test]
fn a_distant_listener_hears_nothing_and_mute_stops_transmission() {
    let _guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let (alice, bob, _alice_io, bob_io) = start_pair(
        Position::new(0, 0.0, 64.0, 0.0),
        Position::new(0, 400.0, 64.0, 0.0),
    );
    thread::sleep(Duration::from_millis(1200));
    assert_eq!(bob_io.levels(), (0.0, 0.0), "out of range must be silent");
    assert!(bob.snapshot().speakers.is_empty());

    // Bring Bob close, then mute Alice: after the buffers drain, nothing new arrives.
    bob.set_local_player(Position::new(0, 4.0, 64.0, 0.0), 0.0);
    alice.set_local_player(Position::new(0, 0.0, 64.0, 0.0), 0.0);
    wait_until("bob hears alice once close", || {
        bob_io.levels().0 + bob_io.levels().1 > 100.0
    });

    alice.set_muted(true);
    thread::sleep(Duration::from_millis(600));
    let before = bob_io.played.lock().unwrap().len();
    thread::sleep(Duration::from_millis(500));
    let after = bob_io.played.lock().unwrap().len();
    assert_eq!(before, after, "muted speaker must not keep producing audio");

    alice.shutdown();
    bob.shutdown();
}

#[test]
fn joining_with_a_bad_or_wrong_code_fails_cleanly() {
    let _guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let bob = VoiceHandle::spawn(FakeIo::new(false), 2002);
    assert!(bob.join_room("not a code").is_err());

    // A well-formed code that points at nothing: the session reports failure, not a hang.
    let code = cinnabar_voice::room::RoomCode::new("127.0.0.1:9".parse().unwrap(), 1).encode();
    bob.join_room(&code).unwrap();
    wait_until("connecting", || bob.snapshot().status == Status::Connecting);
    // The session retries for 10 s before reporting failure.
    wait_until_for("gives up", Duration::from_secs(15), || {
        matches!(bob.snapshot().status, Status::Failed(_))
    });
    bob.shutdown();
}
