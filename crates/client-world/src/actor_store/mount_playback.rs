//! Jitter-buffered playback of a rail-driven cart's streamed positions.
//!
//! A cart streams one position per server tick, yet the frames that apply those packets see
//! them a frame or two early or late. Easing each position over a fixed three actor ticks
//! then speeds the cart up and slows it down by a third whenever a packet lands on the other
//! side of a tick boundary. Playback instead stamps each position with the server tick it
//! belongs to, locks those stamps to the arrivals' steadiest phase, and draws the cart a
//! fixed delay behind them, so the drawn cart keeps the server's steady pace.
//!
//! The playback only chooses what is drawn. Pick states and the positions a rider sends stay
//! on the actor store's own interpolation.

use std::collections::VecDeque;

use super::ActorStore;

/// Server ticks the drawn cart trails the phase-locked arrival of a position; it covers the
/// frames a packet can arrive late and matches the lag of the three-tick easing it replaces.
const PLAYBACK_DELAY_TICKS: f64 = 2.0;
/// Positions kept; the delay needs only the last few.
const MAX_SAMPLES: usize = 12;
/// Arrivals whose earliest phase locks the stamps. The earliest arrival is the one the
/// network and frame timing delayed least.
const PHASE_WINDOW: usize = 24;
/// A silence this long, in ticks, ends the playback: the cart stopped or the link stalled.
const RESET_GAP_TICKS: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Sample {
    /// The server tick this position belongs to, on the actor clock.
    stamp: f64,
    position: [f32; 3],
}

/// The recent positions of one cart and the phase that places them on the actor clock.
#[derive(Debug, Clone, Default)]
pub(super) struct MountPlayback {
    samples: VecDeque<Sample>,
    phases: VecDeque<f64>,
    /// Server ticks since the first retained arrival, counting skipped ticks.
    ordinal: f64,
    last_arrival: f64,
}

impl MountPlayback {
    /// Forgets every position, as a teleport or a long silence ends the playback.
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    /// Records a position that arrived at `now`, in ticks on the actor clock.
    pub(super) fn record(&mut self, now: f64, position: [f32; 3]) {
        if !now.is_finite() || position.iter().any(|axis| !axis.is_finite()) {
            return;
        }
        if self.samples.is_empty() || now - self.last_arrival > RESET_GAP_TICKS {
            self.clear();
        } else {
            let base = self.phase();
            // Delays only add to the phase, so an arrival less than a tick past its tick's
            // earliest phase carries that tick. It never reuses a tick already carried.
            self.ordinal = (self.ordinal + 1.0).max((now - base).floor());
        }
        self.last_arrival = now;
        self.phases.push_back(now - self.ordinal);
        if self.phases.len() > PHASE_WINDOW {
            self.phases.pop_front();
        }
        self.samples.push_back(Sample {
            stamp: self.phase() + self.ordinal,
            position,
        });
        if self.samples.len() > MAX_SAMPLES {
            self.samples.pop_front();
        }
    }

    /// The earliest phase of the recent arrivals.
    fn phase(&self) -> f64 {
        self.phases.iter().copied().fold(f64::INFINITY, f64::min)
    }

    /// Where the cart draws at `now`: its positions eased to the instant
    /// [`PLAYBACK_DELAY_TICKS`] ago, holding the newest once playback has caught up with it.
    pub(super) fn position_at(&self, now: f64) -> Option<[f32; 3]> {
        let at = now - PLAYBACK_DELAY_TICKS;
        let first = self.samples.front()?;
        if at <= first.stamp {
            return Some(first.position);
        }
        let mut earlier = *first;
        for later in self.samples.iter().skip(1) {
            if at <= later.stamp {
                let fraction = ((at - earlier.stamp) / (later.stamp - earlier.stamp)) as f32;
                return Some(std::array::from_fn(|axis| {
                    earlier.position[axis]
                        + (later.position[axis] - earlier.position[axis]) * fraction
                }));
            }
            earlier = *later;
        }
        Some(earlier.position)
    }
}

impl ActorStore {
    /// Draws the cart `rider_unique_id` rides from its playback at `partial_tick` of the way
    /// through the actor clock's tick, and every other cart from its eased ticks. Call it once
    /// per frame after staging the frame's motion; the rider's seat and the cart then draw from
    /// the same position.
    pub(crate) fn play_back_mounts(&mut self, partial_tick: f32, rider_unique_id: i64) {
        self.motion_partial = f64::from(partial_tick.clamp(0.0, 1.0));
        if let Some(runtime_id) = self.played_back_mount.take()
            && let Some(actor) = self.actors.get_mut(&runtime_id)
        {
            actor.status.render_position = None;
        }
        let actors = &self.actors;
        self.mount_playback
            .retain(|runtime_id, _| actors.contains_key(runtime_id));
        let Some(ridden) = self.ridden_unique_id(rider_unique_id) else {
            return;
        };
        // A cart that is itself seated follows its own mount.
        if self.rider_to_ridden.contains_key(&ridden) {
            return;
        }
        let Some(&runtime_id) = self.unique_to_runtime.get(&ridden) else {
            return;
        };
        let now = self.motion_ticks as f64 + self.motion_partial;
        let Some(position) = self
            .mount_playback
            .get(&runtime_id)
            .and_then(|playback| playback.position_at(now))
        else {
            return;
        };
        if let Some(actor) = self.actors.get_mut(&runtime_id)
            && actor.is_minecart()
        {
            actor.status.render_position = Some(position);
            self.played_back_mount = Some(runtime_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds one position per server tick, `arrivals[i]` ticks after tick `i`, and returns the
    /// distance the cart draws per frame of `frames_per_tick` over the steady stretch.
    fn frame_steps(arrivals: &[f64], frames_per_tick: u32) -> Vec<f32> {
        let mut playback = MountPlayback::default();
        let mut index = 0;
        let mut previous = None;
        let mut steps = Vec::new();
        for frame in 0..(arrivals.len() as u32 * frames_per_tick) {
            let now = f64::from(frame) / f64::from(frames_per_tick);
            while index < arrivals.len() && index as f64 + arrivals[index] <= now {
                playback.record(now, [index as f32 * 0.4, 0.0, 0.0]);
                index += 1;
            }
            if let Some(position) = playback.position_at(now) {
                if let Some(previous) = previous.filter(|_| frame > 40 && frame < 150) {
                    steps.push(position[0] - previous);
                }
                previous = Some(position[0]);
            }
        }
        steps
    }

    /// Whole-frame jitter in when each tick's position arrives leaves the drawn cart at the
    /// server's pace.
    #[test]
    fn arrival_jitter_does_not_change_the_drawn_pace() {
        let jitter = [0.0, 0.33, 0.0, 0.17, 0.33, 0.0, 0.5, 0.17, 0.0, 0.33];
        let arrivals: Vec<f64> = (0..60).map(|tick| jitter[tick % jitter.len()]).collect();
        let steps = frame_steps(&arrivals, 6);
        let pace = 0.4 / 6.0;
        assert!(steps.len() > 60);
        for step in steps {
            assert!(
                (step - pace).abs() < pace * 0.1,
                "step {step} against {pace}"
            );
        }
    }

    /// A cart that stopped sending holds its last position, and a new arrival after a long
    /// silence starts a fresh playback instead of sweeping across the gap.
    #[test]
    fn a_silent_cart_holds_and_restarts() {
        let mut playback = MountPlayback::default();
        for tick in 0..5 {
            playback.record(f64::from(tick), [tick as f32, 0.0, 0.0]);
        }
        assert_eq!(playback.position_at(40.0), Some([4.0, 0.0, 0.0]));
        playback.record(50.0, [9.0, 0.0, 0.0]);
        assert_eq!(playback.position_at(50.0), Some([9.0, 0.0, 0.0]));
    }

    /// Positions that are not finite are ignored.
    #[test]
    fn non_finite_positions_are_ignored() {
        let mut playback = MountPlayback::default();
        playback.record(0.0, [f32::NAN, 0.0, 0.0]);
        playback.record(f64::NAN, [0.0; 3]);
        assert_eq!(playback.position_at(5.0), None);
    }
}
