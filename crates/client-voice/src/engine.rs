//! The voice session: one background thread that moves microphone audio to the room and
//! room audio to the speakers. The game talks to it only through a [`VoiceHandle`].
//!
//! Audio hardware sits behind [`AudioIo`], so the whole pipeline is tested with fake devices.

use crate::adpcm::AdpcmCodec;
use crate::client::{ClientState, Incoming, VoiceClient};
use crate::codec::{FRAME_SAMPLES, FrameCodec};
use crate::gate::{NoiseGate, rms};
use crate::jitter::{JitterBuffer, Playout};
use crate::proto::Position;
use crate::relay::{RelayConfig, run_udp};
use crate::room::{RoomCode, generate_secret};
use crate::spatial::{Listener, spatialize};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 24454;
const TICK: Duration = Duration::from_millis(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const SPEAKER_IDLE: Duration = Duration::from_secs(2);
const KEEPALIVE_EVERY: Duration = Duration::from_secs(1);
/// Captured audio older than this many frames is dropped instead of delaying speech.
const MAX_MIC_BACKLOG_FRAMES: usize = 8;

/// Microphone and speakers. Implemented by the cpal backend and by test doubles.
pub trait AudioIo: Send + 'static {
    /// Appends captured 48 kHz mono samples to `out` (does not block).
    fn read_mic(&mut self, out: &mut Vec<i16>);
    /// Queues interleaved 48 kHz stereo samples for playback (does not block).
    fn write_playback(&mut self, stereo: &[i16]);
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Off,
    Connecting,
    Connected,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerInfo {
    pub id: u64,
    /// Loudness of the last decoded frame, `0.0..=1.0`.
    pub level: f32,
    pub volume: f32,
}

/// Everything the interface needs to draw, copied out of the session thread.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub status: Status,
    /// Shown to the host so friends can join. Present while hosting.
    pub room_code: Option<String>,
    pub hosting: bool,
    pub muted: bool,
    pub deafened: bool,
    pub push_to_talk: bool,
    pub master_volume: f32,
    pub mic_level: f32,
    pub transmitting: bool,
    pub speakers: Vec<SpeakerInfo>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            status: Status::Off,
            room_code: None,
            hosting: false,
            muted: false,
            deafened: false,
            push_to_talk: false,
            master_volume: 1.0,
            mic_level: 0.0,
            transmitting: false,
            speakers: Vec::new(),
        }
    }
}

#[derive(Debug)]
enum Command {
    Host {
        port: u16,
        advertise: Option<Ipv4Addr>,
        range: f32,
    },
    Join(RoomCode),
    Leave,
    Muted(bool),
    Deafened(bool),
    PushToTalk(bool),
    Talking(bool),
    MasterVolume(f32),
    PlayerVolume(u64, f32),
    Shutdown,
}

#[derive(Debug, Clone, Copy)]
struct Local {
    pos: Position,
    yaw_deg: f32,
}

/// Cheap, cloneable remote control for the session thread.
#[derive(Clone)]
pub struct VoiceHandle {
    commands: Sender<Command>,
    snapshot: Arc<Mutex<Snapshot>>,
    local: Arc<Mutex<Option<Local>>>,
    worker: Arc<Mutex<Option<JoinHandle<()>>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl VoiceHandle {
    /// Starts the session thread. `player_id` is the player's XUID.
    pub fn spawn(io: impl AudioIo, player_id: u64) -> VoiceHandle {
        let (tx, rx) = mpsc::channel();
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let local = Arc::new(Mutex::new(None));
        let mut session = Session::new(io, player_id, rx, snapshot.clone(), local.clone());
        let worker = thread::Builder::new()
            .name("cinnabar-voice".into())
            .spawn(move || session.run())
            .expect("spawn voice thread");
        VoiceHandle {
            commands: tx,
            snapshot,
            local,
            worker: Arc::new(Mutex::new(Some(worker))),
        }
    }

    /// Call every frame (or at least every few ticks) with the local player's pose.
    pub fn set_local_player(&self, pos: Position, yaw_deg: f32) {
        *lock(&self.local) = Some(Local { pos, yaw_deg });
    }

    pub fn snapshot(&self) -> Snapshot {
        lock(&self.snapshot).clone()
    }

    /// Opens a room on this machine. `advertise` is the address friends should use (your
    /// public IP for the internet, or leave `None` for the LAN address).
    pub fn host_room(&self, advertise: Option<Ipv4Addr>) {
        self.send(Command::Host {
            port: DEFAULT_PORT,
            advertise,
            range: 48.0,
        });
    }

    pub fn join_room(&self, code: &str) -> Result<(), crate::room::RoomCodeError> {
        let code = RoomCode::decode(code)?;
        self.send(Command::Join(code));
        Ok(())
    }

    pub fn leave(&self) {
        self.send(Command::Leave);
    }
    pub fn set_muted(&self, muted: bool) {
        self.send(Command::Muted(muted));
    }
    pub fn set_deafened(&self, deafened: bool) {
        self.send(Command::Deafened(deafened));
    }
    pub fn set_push_to_talk(&self, on: bool) {
        self.send(Command::PushToTalk(on));
    }
    /// The push-to-talk key is held (only matters with push-to-talk on).
    pub fn set_talking(&self, held: bool) {
        self.send(Command::Talking(held));
    }
    pub fn set_master_volume(&self, volume: f32) {
        self.send(Command::MasterVolume(volume));
    }
    pub fn set_player_volume(&self, player_id: u64, volume: f32) {
        self.send(Command::PlayerVolume(player_id, volume));
    }

    /// Stops the session thread and waits for it.
    pub fn shutdown(&self) {
        self.send(Command::Shutdown);
        if let Some(worker) = lock(&self.worker).take() {
            let _ = worker.join();
        }
    }

    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

struct Speaker {
    jitter: JitterBuffer<Vec<u8>>,
    codec: AdpcmCodec,
    pos: Position,
    level: f32,
    last_heard: Instant,
}

struct Hosting {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Hosting {
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Connection {
    client: VoiceClient,
    started: Instant,
    last_hello: Instant,
    last_keepalive: Instant,
    host: Option<Hosting>,
}

struct Session<I: AudioIo> {
    io: I,
    player_id: u64,
    commands: Receiver<Command>,
    shared: Arc<Mutex<Snapshot>>,
    local: Arc<Mutex<Option<Local>>>,
    state: Snapshot,
    conn: Option<Connection>,
    encoder: AdpcmCodec,
    gate: NoiseGate,
    mic: Vec<i16>,
    talking_key: bool,
    speakers: HashMap<u64, Speaker>,
    volumes: HashMap<u64, f32>,
}

impl<I: AudioIo> Session<I> {
    fn new(
        io: I,
        player_id: u64,
        commands: Receiver<Command>,
        shared: Arc<Mutex<Snapshot>>,
        local: Arc<Mutex<Option<Local>>>,
    ) -> Self {
        Self {
            io,
            player_id,
            commands,
            shared,
            local,
            state: Snapshot::default(),
            conn: None,
            encoder: AdpcmCodec::default(),
            gate: NoiseGate::default_voice(),
            mic: Vec::new(),
            talking_key: false,
            speakers: HashMap::new(),
            volumes: HashMap::new(),
        }
    }

    fn run(&mut self) {
        let mut next = Instant::now();
        loop {
            if !self.drain_commands() {
                break;
            }
            let now = Instant::now();
            self.tick(now);
            self.publish();
            next += TICK;
            match next.checked_duration_since(Instant::now()) {
                Some(wait) => thread::sleep(wait),
                None => next = Instant::now(), // fell behind: do not try to catch up
            }
        }
        self.disconnect();
        self.publish();
    }

    /// Returns false when asked to shut down.
    fn drain_commands(&mut self) -> bool {
        loop {
            match self.commands.try_recv() {
                Ok(command) => {
                    if !self.apply(command) {
                        return false;
                    }
                }
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
    }

    fn apply(&mut self, command: Command) -> bool {
        match command {
            Command::Shutdown => return false,
            Command::Host {
                port,
                advertise,
                range,
            } => self.host(port, advertise, range),
            Command::Join(code) => self.join(code),
            Command::Leave => self.disconnect(),
            Command::Muted(v) => self.state.muted = v,
            Command::Deafened(v) => self.state.deafened = v,
            Command::PushToTalk(v) => self.state.push_to_talk = v,
            Command::Talking(v) => self.talking_key = v,
            Command::MasterVolume(v) => self.state.master_volume = v.clamp(0.0, 2.0),
            Command::PlayerVolume(id, v) => {
                self.volumes.insert(id, v.clamp(0.0, 2.0));
            }
        }
        true
    }

    fn fail(&mut self, why: impl Into<String>) {
        self.disconnect();
        self.state.status = Status::Failed(why.into());
    }

    fn host(&mut self, port: u16, advertise: Option<Ipv4Addr>, range: f32) {
        self.disconnect();
        let socket = match UdpSocket::bind(("0.0.0.0", port)) {
            Ok(s) => s,
            Err(e) => return self.fail(format!("no se pudo abrir el puerto {port}: {e}")),
        };
        let secret = generate_secret();
        let Some(ip) = advertise.or_else(lan_address) else {
            return self.fail("no se pudo detectar tu dirección IP");
        };
        let code = RoomCode::new(SocketAddrV4::new(ip, port), secret).encode();

        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let mut cfg = RelayConfig::new(secret);
        cfg.range = range;
        let thread = thread::Builder::new()
            .name("cinnabar-voice-relay".into())
            .spawn(move || {
                let _ = run_udp(socket, cfg, &flag);
            })
            .ok();
        let mut hosting = Hosting { stop, thread };

        let loopback = SocketAddr::from(([127, 0, 0, 1], port));
        match VoiceClient::connect(loopback, secret, self.player_id) {
            Ok(client) => {
                let now = Instant::now();
                self.conn = Some(Connection {
                    client,
                    started: now,
                    last_hello: now,
                    last_keepalive: now,
                    host: Some(hosting),
                });
                self.state.status = Status::Connecting;
                self.state.hosting = true;
                self.state.room_code = Some(code);
            }
            Err(e) => {
                hosting.stop();
                self.fail(format!("no se pudo conectar a tu propia sala: {e}"));
            }
        }
    }

    fn join(&mut self, code: RoomCode) {
        self.disconnect();
        match VoiceClient::connect(code.socket_addr(), code.secret, self.player_id) {
            Ok(client) => {
                let now = Instant::now();
                self.conn = Some(Connection {
                    client,
                    started: now,
                    last_hello: now,
                    last_keepalive: now,
                    host: None,
                });
                self.state.status = Status::Connecting;
            }
            Err(e) => self.fail(format!("no se pudo conectar: {e}")),
        }
    }

    fn disconnect(&mut self) {
        if let Some(mut conn) = self.conn.take() {
            conn.client.leave();
            if let Some(host) = conn.host.as_mut() {
                host.stop();
            }
        }
        self.speakers.clear();
        self.state.status = Status::Off;
        self.state.hosting = false;
        self.state.room_code = None;
        self.state.transmitting = false;
        self.state.mic_level = 0.0;
    }

    fn local_pose(&self) -> Option<Local> {
        *lock(&self.local)
    }

    fn tick(&mut self, now: Instant) {
        self.capture(now);
        self.receive(now);
        self.play(now);
        self.housekeeping(now);
    }

    fn capture(&mut self, now: Instant) {
        self.io.read_mic(&mut self.mic);
        let backlog = MAX_MIC_BACKLOG_FRAMES * FRAME_SAMPLES;
        if self.mic.len() > backlog {
            let excess = self.mic.len() - backlog;
            self.mic.drain(..excess);
        }
        let pose = self.local_pose();
        let mut transmitting = false;
        while self.mic.len() >= FRAME_SAMPLES {
            let frame: Vec<i16> = self.mic.drain(..FRAME_SAMPLES).collect();
            self.state.mic_level = rms(&frame);
            let may_talk = !self.state.muted && (!self.state.push_to_talk || self.talking_key);
            // The gate always sees the frame so its hang time stays accurate.
            let open = self.gate.process(&frame);
            let (Some(conn), Some(pose)) = (self.conn.as_mut(), pose) else {
                continue;
            };
            if conn.client.state() != ClientState::Connected || !may_talk {
                continue;
            }
            // Push-to-talk bypasses the gate: the key already says "I am talking".
            if (open || self.state.push_to_talk)
                && let Ok(payload) = self.encoder.encode(&frame)
                && conn.client.send_voice(pose.pos, &payload).is_ok()
            {
                conn.last_keepalive = now;
                transmitting = true;
            }
        }
        self.state.transmitting = transmitting;
        if self.mic.is_empty() && !transmitting {
            // keep the meter from sticking when nothing arrives
            self.state.mic_level *= 0.8;
        }
    }

    fn receive(&mut self, now: Instant) {
        let Some(conn) = self.conn.as_mut() else {
            return;
        };
        let events = match conn.client.poll() {
            Ok(events) => events,
            Err(e) => return self.fail(format!("se perdió la conexión: {e}")),
        };
        for event in events {
            match event {
                Incoming::Connected { .. } => self.state.status = Status::Connected,
                Incoming::Denied => {
                    return self.fail("la sala rechazó la conexión (código incorrecto o llena)");
                }
                Incoming::Voice {
                    speaker,
                    seq,
                    pos,
                    payload,
                } => {
                    let entry = self.speakers.entry(speaker).or_insert_with(|| Speaker {
                        jitter: JitterBuffer::new(3, 10),
                        codec: AdpcmCodec::default(),
                        pos,
                        level: 0.0,
                        last_heard: now,
                    });
                    entry.pos = pos;
                    entry.last_heard = now;
                    entry.jitter.push(seq, payload);
                }
            }
        }
    }

    fn play(&mut self, _now: Instant) {
        let range = self.conn.as_ref().map_or(0.0, |c| c.client.range());
        let listener = self.local_pose().map(|l| Listener {
            pos: l.pos,
            yaw_deg: l.yaw_deg,
        });
        let master = if self.state.deafened {
            0.0
        } else {
            self.state.master_volume
        };
        let mut mix = vec![0f32; FRAME_SAMPLES * 2];
        let mut any = false;
        for (id, speaker) in &mut self.speakers {
            let decoded = match speaker.jitter.pop() {
                Playout::Frame(data) => speaker.codec.decode(Some(&data)),
                Playout::Missing => speaker.codec.decode(None),
                Playout::Waiting => continue,
            };
            let Ok(pcm) = decoded else { continue };
            speaker.level = rms(&pcm);
            let gains = match listener {
                Some(l) => spatialize(&l, &speaker.pos, range).map(|s| (s.left, s.right)),
                // No pose yet (menus): play centered rather than silently dropping.
                None => Some((0.707, 0.707)),
            };
            let Some((left, right)) = gains else { continue };
            let volume = master * self.volumes.get(id).copied().unwrap_or(1.0);
            for (i, &sample) in pcm.iter().enumerate() {
                let s = f32::from(sample) / 32768.0 * volume;
                mix[i * 2] += s * left;
                mix[i * 2 + 1] += s * right;
            }
            any = true;
        }
        if any {
            let out: Vec<i16> = mix
                .iter()
                .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
                .collect();
            self.io.write_playback(&out);
        }
    }

    fn housekeeping(&mut self, now: Instant) {
        self.speakers
            .retain(|_, s| now.duration_since(s.last_heard) < SPEAKER_IDLE);
        let pose = self.local_pose();
        let Some(conn) = self.conn.as_mut() else {
            return;
        };
        match conn.client.state() {
            ClientState::Connecting => {
                if now.duration_since(conn.started) > CONNECT_TIMEOUT {
                    return self.fail("la sala no responde (¿puerto abierto y código correcto?)");
                }
                if now.duration_since(conn.last_hello) >= Duration::from_secs(1) {
                    let _ = conn.client.send_hello();
                    conn.last_hello = now;
                }
            }
            ClientState::Connected => {
                if now.duration_since(conn.last_keepalive) >= KEEPALIVE_EVERY {
                    if let Some(p) = pose {
                        let _ = conn.client.send_keepalive(p.pos);
                    }
                    conn.last_keepalive = now;
                }
            }
            ClientState::Denied => {}
        }
    }

    fn publish(&mut self) {
        self.state.speakers = self
            .speakers
            .iter()
            .map(|(id, s)| SpeakerInfo {
                id: *id,
                level: s.level,
                volume: self.volumes.get(id).copied().unwrap_or(1.0),
            })
            .collect();
        self.state.speakers.sort_by_key(|s| s.id);
        *lock(&self.shared) = self.state.clone();
    }
}

/// The machine's LAN address, found by asking the OS which interface routes outward.
/// Nothing is sent: connecting a UDP socket only selects a route.
fn lan_address() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}
