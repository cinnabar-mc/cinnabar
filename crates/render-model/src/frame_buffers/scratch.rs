//! Capacity policy for frame scratch and retained GPU buffers.

use std::ops::{Deref, DerefMut};

/// Keeps capacity through short fluctuations and releases a sustained oversized peak.
#[derive(Clone, Copy, Debug, Default)]
pub struct CapacityRetention {
    low_frames: usize,
}

impl CapacityRetention {
    /// Consecutive low-use frames required before trimming storage.
    pub const SHRINK_AFTER: usize = 120;
    const HEADROOM: usize = 4;

    /// Returns geometric growth immediately or a smaller capacity after sustained low use.
    /// This is a storage policy only; it never limits the live element count.
    pub fn target(&mut self, capacity: usize, len: usize, minimum: usize) -> usize {
        let needed = len.max(minimum);
        if needed > capacity {
            self.low_frames = 0;
            return needed.next_power_of_two();
        }
        if capacity > needed.saturating_mul(Self::HEADROOM) {
            self.low_frames += 1;
            if self.low_frames >= Self::SHRINK_AFTER {
                self.low_frames = 0;
                return if needed == 0 {
                    0
                } else {
                    needed.next_power_of_two()
                };
            }
        } else {
            self.low_frames = 0;
        }
        capacity
    }
}

/// A reusable vector whose spare storage shrinks only after sustained low use.
#[derive(Debug)]
pub struct FrameScratch<T> {
    values: Vec<T>,
    retention: CapacityRetention,
    peak: usize,
}

impl<T> Default for FrameScratch<T> {
    /// Starts empty without reserving storage.
    fn default() -> Self {
        Self {
            values: Vec::new(),
            retention: CapacityRetention::default(),
            peak: 0,
        }
    }
}

impl<T> FrameScratch<T> {
    /// Records live usage before another command clears and reuses this frame's vector.
    pub fn record_usage(&mut self) {
        self.peak = self.peak.max(self.values.len());
    }

    /// Accounts for this frame's completed output and releases sustained excess capacity.
    /// Repeated commands call `record_usage`; the frame owner calls this once for their peak.
    pub fn finish_frame(&mut self) {
        let used = std::mem::take(&mut self.peak).max(self.values.len());
        let target = self.retention.target(self.values.capacity(), used, 0);
        if target < self.values.capacity() {
            self.values.shrink_to(target);
        }
    }
}

impl<T> Deref for FrameScratch<T> {
    type Target = Vec<T>;

    /// Exposes the live vector without transferring its retained allocation.
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl<T> DerefMut for FrameScratch<T> {
    /// Allows the owner to rebuild this frame in the same vector.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.values
    }
}
