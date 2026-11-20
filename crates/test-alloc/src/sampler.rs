//! Byte-weighted allocation sampling for the heap profiler.
//!
//! Each sample stands for [`Sampler::mean`] bytes. The gap between samples is drawn from an
//! exponential distribution with that mean, so a workload whose allocation sizes repeat on a
//! fixed period cannot line its samples up with only short-lived (or only retained) blocks.

/// Allocations at least this large are always recorded with their exact size.
pub const LARGE_BYTES: usize = 64 << 10;
/// Smaller allocations are sampled on average once per this many bytes on each thread.
pub const SAMPLE_BYTES: u64 = 256 << 10;

/// Chooses which small allocations a thread records.
#[derive(Clone, Debug)]
pub struct Sampler {
    mean: u64,
    /// Bytes left before the next sample.
    countdown: u64,
    /// xorshift64* state; never zero.
    state: u64,
}

impl Sampler {
    /// A sampler averaging one sample per `mean` bytes, with a pseudo-random sequence fixed
    /// by `seed`.
    #[must_use]
    pub fn new(mean: u64, seed: u64) -> Self {
        let mut sampler = Self {
            mean: mean.max(1),
            countdown: 0,
            state: seed | 1,
        };
        sampler.countdown = sampler.gap();
        sampler
    }

    /// The bytes each sample stands for.
    #[must_use]
    pub fn mean(&self) -> u64 {
        self.mean
    }

    /// How many sampling points an allocation of `size` bytes crosses; record it with that
    /// many times [`Self::mean`] bytes. Zero means it is not sampled.
    pub fn sample(&mut self, size: usize) -> u64 {
        let mut left = size as u64;
        let mut points = 0;
        while left >= self.countdown {
            left -= self.countdown;
            self.countdown = self.gap();
            points += 1;
        }
        self.countdown -= left;
        points
    }

    /// The next exponentially distributed gap, at least one byte.
    fn gap(&mut self) -> u64 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        let bits = self.state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11;
        // Uniform in (0, 1]: never zero, so the logarithm stays finite.
        let uniform = (bits + 1) as f64 / (1_u64 << 53) as f64;
        ((-uniform.ln() * self.mean as f64) as u64).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retained and freed blocks of one size alternate; the samples of the retained ones must
    /// still estimate the retained total.
    #[test]
    fn periodic_retained_and_freed_blocks_are_estimated_without_aliasing() {
        const SIZE: usize = 32 << 10;
        const MEAN: u64 = SAMPLE_BYTES;
        const PAIRS: usize = 8192;
        let mut sampler = Sampler::new(MEAN, 0x5eed);
        let mut estimated = 0_u64;
        for _ in 0..PAIRS {
            estimated += sampler.sample(SIZE) * sampler.mean();
            // Its freed partner: a sample here is forgotten when the block is freed.
            sampler.sample(SIZE);
        }
        let retained = (PAIRS * SIZE) as f64;
        let error = (estimated as f64 - retained).abs() / retained;
        assert!(error < 0.15, "estimated {estimated} of {retained} bytes");
    }

    /// Blocks near the size above which every allocation is recorded exactly still count in
    /// full: one crossing several sampling points carries all of them.
    #[test]
    fn blocks_spanning_several_sampling_points_keep_their_full_weight() {
        const SIZE: usize = LARGE_BYTES - (4 << 10);
        const MEAN: u64 = SAMPLE_BYTES;
        const BLOCKS: usize = 16_384;
        let mut sampler = Sampler::new(MEAN, 0xb10c);
        let estimated: u64 = (0..BLOCKS).map(|_| sampler.sample(SIZE) * MEAN).sum();
        let retained = (BLOCKS * SIZE) as f64;
        let error = (estimated as f64 - retained).abs() / retained;
        assert!(error < 0.05, "estimated {estimated} of {retained} bytes");
        let mut tiny = Sampler::new(16, 3);
        assert!(
            tiny.sample(1 << 20) > 1,
            "one large block spans many points"
        );
    }
}
