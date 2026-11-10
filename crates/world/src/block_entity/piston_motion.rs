//! Client-side piston arm motion.
//!
//! A native client ignores the `Progress` an arm actor's data reports. It keeps its own
//! progress, which starts at zero for a new actor, and moves it half a block per tick toward
//! the end of the reported `State`; an update keeps the progress it reached.

#[cfg(test)]
mod tests;

/// Native `PistonBlockActor` states as the actor data's `State` byte reports them.
const RETRACTED: u8 = 0;
const EXPANDING: u8 = 1;
const EXPANDED: u8 = 2;
const RETRACTING: u8 = 3;

/// Half-block steps of a fully extended arm.
const EXTENDED_STEPS: u8 = 2;

/// Ticks after which every motion has reached its end and stopped.
const SETTLE_TICKS: u64 = EXTENDED_STEPS as u64;

/// A piston arm's client progress, as a function of the client simulation tick.
///
/// The progress after `anchor_tick` is `anchor_steps` half blocks; each later tick applies
/// the native client update for `state` once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PistonMotion {
    state: u8,
    anchor_tick: u64,
    anchor_steps: u8,
}

/// An arm's client progress after one tick's update, and before it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PistonStep {
    pub progress: f32,
    pub last_progress: f32,
}

impl PistonStep {
    /// Whether the tick's update moved the arm, which is when native pushes entities.
    #[must_use]
    pub fn moved(self) -> bool {
        self.progress != self.last_progress
    }
}

impl PistonMotion {
    /// A new actor's motion before the store anchors it: retracted, reporting `state`.
    pub(crate) const fn reported_state(state: u8) -> Self {
        Self {
            state,
            anchor_tick: 0,
            anchor_steps: 0,
        }
    }

    /// The state the server last reported.
    pub(crate) const fn state(&self) -> u8 {
        self.state
    }

    /// The motion after `state` arrives once the client has simulated `tick`.
    ///
    /// Native load replaces only the state, so an update that keeps the state keeps the
    /// motion and one that changes it continues from the progress already reached. A new
    /// actor starts retracted.
    pub(crate) fn reported(previous: Option<&Self>, state: u8, tick: u64) -> Self {
        match previous {
            Some(previous) if previous.state == state => *previous,
            Some(previous) => Self {
                state,
                anchor_tick: tick,
                anchor_steps: previous.advanced(tick).1,
            },
            None => Self {
                state,
                anchor_tick: tick,
                anchor_steps: 0,
            },
        }
    }

    /// The same motion re-anchored at `tick` with the progress and state it had after `from`,
    /// for a simulation clock that restarted below its anchor.
    pub(crate) fn rebased(&self, from: u64, tick: u64) -> Self {
        let (state, steps, _) = self.advanced(from);
        Self {
            state,
            anchor_tick: tick,
            anchor_steps: steps,
        }
    }

    /// The first tick after which the progress no longer changes.
    pub(crate) const fn settled_after(&self) -> u64 {
        self.anchor_tick.saturating_add(SETTLE_TICKS)
    }

    /// The progress after `tick`'s update and before it.
    #[must_use]
    pub fn step_at(&self, tick: u64) -> PistonStep {
        let (_, steps, last) = self.advanced(tick);
        PistonStep {
            progress: f32::from(steps) * 0.5,
            last_progress: f32::from(last) * 0.5,
        }
    }

    /// The local state, the steps after `tick`'s update and the steps before it.
    fn advanced(&self, tick: u64) -> (u8, u8, u8) {
        let (mut state, mut steps) = (self.state, self.anchor_steps);
        let mut last = steps;
        // One tick past settling repeats the end, so the step before it equals the step after.
        for _ in 0..tick.saturating_sub(self.anchor_tick).min(SETTLE_TICKS + 1) {
            last = steps;
            (state, steps) = client_update(state, steps);
        }
        (state, steps, last)
    }
}

/// One native client update (`PistonBlockActor::tick` when the level is client side): an
/// expanding arm gains and a retracting arm loses half a block until it reaches its end, then
/// takes the resting state; a resting arm snaps to its end. Unknown states hold.
const fn client_update(state: u8, steps: u8) -> (u8, u8) {
    match state {
        RETRACTED => (RETRACTED, 0),
        EXPANDED => (EXPANDED, EXTENDED_STEPS),
        EXPANDING => {
            let steps = if steps < EXTENDED_STEPS {
                steps + 1
            } else {
                steps
            };
            if steps >= EXTENDED_STEPS {
                (EXPANDED, EXTENDED_STEPS)
            } else {
                (EXPANDING, steps)
            }
        }
        RETRACTING => {
            let steps = steps.saturating_sub(1);
            if steps == 0 {
                (RETRACTED, 0)
            } else {
                (RETRACTING, steps)
            }
        }
        _ => (state, steps),
    }
}
