//! Numeric diagnostics retained only by an opt-in packet capture.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

/// A frozen pick and the input boundaries that consumed it. Zero sequences mean unavailable.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PickState {
    pub pick_frame: [u64; 2],
    pub press_frame: [u64; 2],
    pub transaction_frame: [u64; 2],
    pub target: u64,
    pub last_movement_sequence: u64,
    pub applied_sequence: u64,
    pub remote_tick: u64,
    pub frame_alpha: Option<f32>,
    pub eye: [f32; 3],
    pub direction: [f32; 3],
    /// The inflated box actually intersected; absent for an empty hover or assisted point.
    pub pick_box: Option<([f32; 3], [f32; 3])>,
    pub previous_position: [f32; 3],
    pub current_position: [f32; 3],
    pub interpolation_steps: u32,
    pub hit: [f32; 3],
    pub assisted: bool,
}

/// Fixed v3 pick payload size, excluding its packet sequence.
pub(super) const PICK_BYTES: usize = 173;

impl PickState {
    /// Encodes fixed-width numeric fields without allocating.
    pub(super) fn encode(&self) -> [u8; PICK_BYTES] {
        let mut out = [0; PICK_BYTES];
        let mut offset = 0;
        let mut put = |bytes: &[u8]| {
            out[offset..offset + bytes.len()].copy_from_slice(bytes);
            offset += bytes.len();
        };
        for value in self
            .pick_frame
            .into_iter()
            .chain(self.press_frame)
            .chain(self.transaction_frame)
            .chain([
                self.target,
                self.last_movement_sequence,
                self.applied_sequence,
                self.remote_tick,
            ])
        {
            put(&value.to_le_bytes());
        }
        put(&self.frame_alpha.unwrap_or_default().to_le_bytes());
        let (min, max) = self.pick_box.unwrap_or_default();
        for vector in [
            self.eye,
            self.direction,
            min,
            max,
            self.previous_position,
            self.current_position,
            self.hit,
        ] {
            for value in vector {
                put(&value.to_le_bytes());
            }
        }
        put(&self.interpolation_steps.to_le_bytes());
        put(&[u8::from(self.frame_alpha.is_some())
            | (u8::from(self.pick_box.is_some()) << 1)
            | (u8::from(self.assisted) << 2)]);
        out
    }

    /// Decodes an exact v3 payload and rejects unknown flags.
    pub(super) fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != PICK_BYTES || bytes[PICK_BYTES - 1] & !7 != 0 {
            return None;
        }
        let mut offset = 0;
        let mut next_u64 = || {
            let value =
                u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("eight bytes"));
            offset += 8;
            value
        };
        let pick_frame = std::array::from_fn(|_| next_u64());
        let press_frame = std::array::from_fn(|_| next_u64());
        let transaction_frame = std::array::from_fn(|_| next_u64());
        let target = next_u64();
        let last_movement_sequence = next_u64();
        let applied_sequence = next_u64();
        let remote_tick = next_u64();
        let mut next_f32 = || {
            let value =
                f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"));
            offset += 4;
            value
        };
        let alpha = next_f32();
        let [
            eye,
            direction,
            min,
            max,
            previous_position,
            current_position,
            hit,
        ] = std::array::from_fn(|_| std::array::from_fn(|_| next_f32()));
        let interpolation_steps =
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"));
        let flags = bytes[PICK_BYTES - 1];
        Some(Self {
            pick_frame,
            press_frame,
            transaction_frame,
            target,
            last_movement_sequence,
            applied_sequence,
            remote_tick,
            frame_alpha: (flags & 1 != 0).then_some(alpha),
            eye,
            direction,
            pick_box: (flags & 2 != 0).then_some((min, max)),
            previous_position,
            current_position,
            interpolation_steps,
            hit,
            assisted: flags & 4 != 0,
        })
    }
}

/// Capture-only correspondence between transport packets, world FIFO entries and actor time.
#[derive(Default)]
pub struct PickTrace {
    stopped: Arc<AtomicBool>,
    world: Mutex<BTreeMap<u64, (u64, bool)>>,
    clock: Mutex<(u64, Option<f32>)>,
}

impl PickTrace {
    /// Shares the capture writer's lifetime so finished captures stop collecting diagnostics.
    pub fn new(stopped: Arc<AtomicBool>) -> Self {
        Self {
            stopped,
            ..Self::default()
        }
    }

    /// Whether the capture writer can still accept diagnostics.
    pub fn active(&self) -> bool {
        !self.stopped.load(Ordering::Acquire)
    }

    /// Associates queued world work with its captured packet while the capture is active.
    pub fn world(&self, sequence: u64, packet: u64) {
        let mut world = self.world.lock().unwrap_or_else(PoisonError::into_inner);
        if self.active() {
            world.insert(sequence, (packet, false));
        }
    }

    /// Resolves and marks a mapping used by this publication; zero means no captured producer.
    pub fn packet_sequence(&self, sequence: u64) -> u64 {
        let mut world = self.world.lock().unwrap_or_else(PoisonError::into_inner);
        world.get_mut(&sequence).map_or(0, |entry| {
            entry.1 = true;
            entry.0
        })
    }

    /// Keeps pending work and mappings read by the just-published frontier and actor poses.
    pub fn retire(&self, frontier: u64) {
        self.world
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|sequence, entry| {
                let used = std::mem::take(&mut entry.1);
                *sequence > frontier || used
            });
    }

    /// Releases correspondence storage on every writer exit and prevents late producers adding it.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.world
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// Counts the remote ticks predicted by the actual actor clock once per frame.
    pub fn advance(&self, ticks: u32, alpha: f32) {
        let mut clock = self.clock.lock().unwrap_or_else(PoisonError::into_inner);
        clock.0 += u64::from(ticks);
        clock.1 = Some(alpha);
    }

    /// Returns the remote simulation endpoint and render fraction at this publication.
    pub fn clock(&self) -> (u64, Option<f32>) {
        *self.clock.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
