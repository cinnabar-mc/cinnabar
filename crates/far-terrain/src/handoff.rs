//! Timed presentation handoffs retain a known predecessor while its ready replacement is drawn.

use std::{collections::HashMap, time::Duration};
use world::ChunkKey;

/// Duration of the optional terrain presentation transition.
pub const HANDOFF_DURATION: Duration = Duration::from_millis(300);
/// Ordered coverage samples per transition; the shader uses a four-by-four pattern.
pub const HANDOFF_STEPS: u32 = 16;
/// Depth ranks follow the coverage bits in the renderer's presentation byte.
pub const HANDOFF_DEPTH_SHIFT: u32 = HANDOFF_STEPS.ilog2() + 1;
/// The packed code reserves its low byte for ordered coverage progress.
pub const HANDOFF_DIRECTION_SHIFT: u32 = 8;
/// The near drawer is the outgoing predecessor.
pub const HANDOFF_NEAR: u32 = 1;
/// The far drawer is the outgoing predecessor.
pub const HANDOFF_FAR: u32 = 2;

/// One ready-to-ready change of drawer, timed by its caller's presentation clock.
#[derive(Clone, Copy, Debug)]
pub struct Handoff {
    started: Duration,
    predecessor_near: bool,
}

impl Handoff {
    /// Retains the selected predecessor until the replacement's transition ends.
    #[must_use]
    pub const fn new(started: Duration, predecessor_near: bool) -> Self {
        Self {
            started,
            predecessor_near,
        }
    }

    /// Quantized coverage removed from the predecessor, from zero through all samples.
    #[must_use]
    pub fn step(self, now: Duration) -> u32 {
        ((now.saturating_sub(self.started).as_nanos() * u128::from(HANDOFF_STEPS))
            / HANDOFF_DURATION.as_nanos())
        .min(u128::from(HANDOFF_STEPS)) as u32
    }

    /// Whether every sample can now use the proven replacement alone.
    #[must_use]
    pub fn complete(self, now: Duration) -> bool {
        now.saturating_sub(self.started) >= HANDOFF_DURATION
    }

    /// Encodes the outgoing drawer and its ordered removal progress for the far shader.
    #[must_use]
    pub fn code(self, now: Duration) -> u32 {
        let direction = if self.predecessor_near {
            HANDOFF_NEAR
        } else {
            HANDOFF_FAR
        };
        direction << HANDOFF_DIRECTION_SHIFT | self.step(now)
    }
}

/// Active per-column visual transitions, separate from the publication's exact readiness proofs.
#[derive(Default)]
pub struct TerrainHandoffs(HashMap<ChunkKey, Handoff>);

impl TerrainHandoffs {
    /// Starts a proven drawer change; reversing one restores its still-retained predecessor.
    pub fn retarget(&mut self, column: ChunkKey, predecessor_near: bool, now: Duration) {
        match self.0.get(&column) {
            Some(active) if active.predecessor_near != predecessor_near => {
                self.0.remove(&column);
            }
            Some(_) => {}
            None => {
                self.0.insert(column, Handoff::new(now, predecessor_near));
            }
        }
    }

    /// Ends completed or out-of-view handoffs without reading a clock itself.
    pub fn advance(&mut self, now: Duration, mut retained: impl FnMut(ChunkKey) -> bool) -> bool {
        let before = self.0.len();
        self.0
            .retain(|column, handoff| retained(*column) && !handoff.complete(now));
        self.0.len() != before
    }

    /// Both drawers remain admitted while a column has an active handoff.
    #[must_use]
    pub fn contains(&self, column: ChunkKey) -> bool {
        self.0.contains_key(&column)
    }

    /// Columns whose two drawers must remain resident during presentation.
    pub fn columns(&self) -> impl Iterator<Item = ChunkKey> + '_ {
        self.0.keys().copied()
    }

    /// Visits active columns and their current packed shader codes.
    pub fn codes(&self, now: Duration) -> impl Iterator<Item = (ChunkKey, u32)> + '_ {
        self.0
            .iter()
            .map(move |(&column, &handoff)| (column, handoff.code(now)))
    }

    /// Reports whether another presentation tick is needed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Drops old-world presentation state immediately at its generation boundary.
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predecessor_lives_until_every_ordered_sample_has_a_replacement() {
        let now = Duration::from_secs(5);
        let column = ChunkKey::new(0, -4, 7);
        for near in [false, true] {
            let mut fades = TerrainHandoffs::default();
            fades.retarget(column, near, now);
            for step in 0..HANDOFF_STEPS {
                let at = now + HANDOFF_DURATION * step / HANDOFF_STEPS;
                assert!(!fades.advance(at, |_| true));
                assert!(fades.contains(column));
                let code = fades.codes(at).next().unwrap().1;
                assert_eq!(code & 255, step);
                let direction = if near { HANDOFF_NEAR } else { HANDOFF_FAR };
                assert_eq!(code >> HANDOFF_DIRECTION_SHIFT, direction);
            }
            assert!(fades.advance(now + HANDOFF_DURATION, |_| true));
            assert!(!fades.contains(column));
        }
    }

    #[test]
    fn reversal_and_generation_retirement_keep_no_orphaned_transition() {
        let now = Duration::from_secs(5);
        let column = ChunkKey::new(0, 1, 2);
        let mut fades = TerrainHandoffs::default();
        fades.retarget(column, true, now);
        fades.retarget(column, false, now + HANDOFF_DURATION / 2);
        assert!(fades.is_empty());
        fades.retarget(column, false, now);
        assert!(fades.advance(now, |_| false));
        assert!(fades.is_empty());
        fades.retarget(column, true, now);
        fades.clear();
        assert!(fades.is_empty());
    }

    /// Restating the same change keeps its progress instead of restarting or cancelling it.
    #[test]
    fn repeated_retarget_keeps_the_original_start() {
        let now = Duration::from_secs(5);
        let column = ChunkKey::new(0, 3, 3);
        let mut fades = TerrainHandoffs::default();
        fades.retarget(column, true, now);
        fades.retarget(column, true, now + HANDOFF_DURATION / 2);
        assert!(fades.contains(column));
        assert!(fades.advance(now + HANDOFF_DURATION, |_| true));
    }
}
