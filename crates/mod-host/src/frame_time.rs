//! Cooperative frame scheduling. Elapsed time changes admission, never the guest fault limit.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// A target for all personal mod callbacks together; host calls can overshoot it.
const FRAME_TIME: Duration = Duration::from_millis(1);
type Clock = Arc<dyn Fn() -> Duration + Send + Sync>;

struct Schedule {
    clock: Clock,
    enabled: bool,
    frame: u64,
    used: Duration,
    credits: BTreeMap<usize, i128>,
    pending: BTreeSet<usize>,
}

/// Shares time accounting with mandatory synchronous calls and every runnable frame owner.
#[derive(Clone)]
pub(crate) struct FrameTime(Arc<Mutex<Schedule>>);

impl Default for FrameTime {
    /// Reads time only at the scheduling boundary; tests supply a manual clock.
    fn default() -> Self {
        let origin = Instant::now();
        Self::with_clock(Arc::new(move || origin.elapsed()))
    }
}

impl FrameTime {
    /// Creates a scheduler with an elapsed-time source independent of guest observations.
    fn with_clock(clock: Clock) -> Self {
        Self(Arc::new(Mutex::new(Schedule {
            clock,
            enabled: false,
            frame: 0,
            used: Duration::ZERO,
            credits: Default::default(),
            pending: Default::default(),
        })))
    }

    /// Advances only on the host's frame boundary, never on a guest self-wake.
    pub(crate) fn reset(&self) {
        let mut state = self.0.lock().expect("mod time budget");
        state.frame = state.frame.wrapping_add(1);
        state.used = Duration::ZERO;
    }

    /// Adds equal shares, carries overshoot debt, and removes departed owners.
    pub(crate) fn configure(&self, owners: &[usize]) {
        let mut state = self.0.lock().expect("mod time budget");
        state.enabled = true;
        state.pending = owners.iter().copied().collect();
        state.credits.retain(|owner, _| owners.contains(owner));
        let share = FRAME_TIME.as_nanos() as i128 / owners.len().max(1) as i128;
        for owner in owners {
            let credit = state.credits.entry(*owner).or_default();
            *credit = (*credit + share).min(FRAME_TIME.as_nanos() as i128);
        }
    }

    /// Whether the production host has installed a frame-wide schedule.
    pub(crate) fn enabled(&self) -> bool {
        self.0.lock().expect("mod time budget").enabled
    }

    /// Samples scheduling time without borrowing a running guest's store.
    pub(crate) fn now(&self) -> Duration {
        let clock = self.0.lock().expect("mod time budget").clock.clone();
        clock()
    }

    /// Accounts synchronous work and cooperative polls against the same total and owner debt.
    pub(crate) fn charge(&self, owner: usize, elapsed: Duration) {
        let mut state = self.0.lock().expect("mod time budget");
        state.used = state.used.saturating_add(elapsed);
        *state.credits.entry(owner).or_default() -= elapsed.as_nanos() as i128;
    }

    /// Returns one owner slice only while both its fair share and the total permit work.
    pub(crate) fn slice(&self, owner: usize) -> Option<Slice> {
        let state = self.0.lock().expect("mod time budget");
        let credit = *state.credits.get(&owner)?;
        if credit <= 0 || state.used >= FRAME_TIME {
            return None;
        }
        let allowance = Duration::from_nanos(credit.min(u64::MAX as i128) as u64)
            .min(FRAME_TIME.saturating_sub(state.used));
        Some(Slice {
            time: self.clone(),
            frame: state.frame,
            allowance,
        })
    }

    /// Redistributes unused shares to owners that have not completed this frame.
    pub(crate) fn completed(&self, owner: usize) {
        let mut state = self.0.lock().expect("mod time budget");
        state.pending.remove(&owner);
        let credit = state.credits.entry(owner).or_default();
        let unused = (*credit).max(0);
        *credit -= unused;
        let share = unused / state.pending.len().max(1) as i128;
        for owner in state.pending.clone() {
            *state.credits.entry(owner).or_default() += share;
        }
    }
}

/// A single host-authorized poll window; self-wakes cannot create another one.
pub(crate) struct Slice {
    pub time: FrameTime,
    pub frame: u64,
    pub allowance: Duration,
}

#[cfg(test)]
impl FrameTime {
    /// Installs a deterministic elapsed-time source without sleeping or reading real time.
    pub(crate) fn fake(clock: impl Fn() -> Duration + Send + Sync + 'static) -> Self {
        Self::with_clock(Arc::new(clock))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_redistribute_and_overshoot_debt_survives_frame_reset() {
        let time = FrameTime::fake(|| Duration::ZERO);
        time.configure(&[0, 1]);
        assert_eq!(time.slice(0).unwrap().allowance, FRAME_TIME / 2);
        time.charge(0, FRAME_TIME / 4);
        time.completed(0);
        assert_eq!(time.slice(1).unwrap().allowance, FRAME_TIME * 3 / 4);
        time.charge(1, FRAME_TIME);
        assert!(time.slice(1).is_none());
        time.reset();
        time.configure(&[0, 1]);
        assert_eq!(time.slice(1).unwrap().allowance, FRAME_TIME / 4);
    }
}
