//! Byte-weighted allocation sampling for the heap profiler.
//!
//! Each sample stands for [`Sampler::mean`] bytes. The gap between samples is drawn from an
//! exponential distribution with that mean, so a workload whose allocation sizes repeat on a
//! fixed period cannot line its samples up with only short-lived (or only retained) blocks.

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

    /// Whether an allocation of `size` bytes is sampled.
    pub fn sample(&mut self, size: usize) -> bool {
        let size = size as u64;
        if size < self.countdown {
            self.countdown -= size;
            return false;
        }
        let mut over = size - self.countdown;
        self.countdown = self.gap();
        // An allocation larger than the next gap still yields one sample; its excess counts
        // toward the following ones.
        while over >= self.countdown {
            over -= self.countdown;
            self.countdown = self.gap();
        }
        self.countdown -= over;
        true
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
        const MEAN: u64 = 256 << 10;
        const PAIRS: usize = 8192;
        let mut sampler = Sampler::new(MEAN, 0x5eed);
        let mut estimated = 0_u64;
        for _ in 0..PAIRS {
            if sampler.sample(SIZE) {
                estimated += sampler.mean();
            }
            // Its freed partner: a sample here is forgotten when the block is freed.
            sampler.sample(SIZE);
        }
        let retained = (PAIRS * SIZE) as f64;
        let error = (estimated as f64 - retained).abs() / retained;
        assert!(error < 0.15, "estimated {estimated} of {retained} bytes");
    }

    #[test]
    fn one_large_allocation_is_one_sample() {
        let mut sampler = Sampler::new(1024, 7);
        assert!(sampler.sample(1 << 20));
        let samples = (0..4096).filter(|_| sampler.sample(16)).count();
        // 64 KiB in 16-byte blocks: about 64 samples at one per KiB.
        assert!((32..128).contains(&samples), "{samples}");
    }
}
