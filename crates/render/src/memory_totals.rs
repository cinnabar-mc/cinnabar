//! GPU byte totals the client's memory lines report, shared across the main and render worlds.
//!
//! The device allocator's totals are sampled only on request, because a report lists every
//! allocation; the terrain arena's capacity is recorded whenever it changes.

use std::sync::{
    Mutex, PoisonError,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

/// Marks a total nothing has reported yet.
const UNKNOWN: u64 = u64::MAX;

static SAMPLE_REQUESTED: AtomicBool = AtomicBool::new(false);
/// The last allocator sample, published whole so readers never pair figures from two samples.
static GPU_TOTALS: Mutex<Option<GpuTotals>> = Mutex::new(None);
static CHUNK_ARENA: AtomicU64 = AtomicU64::new(UNKNOWN);

/// What the device allocator held at its last requested sample.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuTotals {
    /// Bytes in live buffers and textures.
    pub allocated: u64,
    /// Bytes of device memory the allocator holds for them, free room included.
    pub reserved: u64,
}

/// Asks the render world to sample the device allocator at the end of its next frame.
pub fn request_gpu_totals() {
    SAMPLE_REQUESTED.store(true, Ordering::Relaxed);
}

/// The last sampled allocator totals; `None` before the first sample and on backends whose
/// allocator reports nothing (Metal, GL).
#[must_use]
pub fn gpu_totals() -> Option<GpuTotals> {
    *GPU_TOTALS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The terrain mesh arena's buffer capacity at its last change; `None` before it exists.
#[must_use]
pub fn chunk_arena_bytes() -> Option<u64> {
    Some(CHUNK_ARENA.load(Ordering::Relaxed)).filter(|bytes| *bytes != UNKNOWN)
}

/// Takes a pending sample request.
pub(crate) fn take_request() -> bool {
    SAMPLE_REQUESTED.swap(false, Ordering::Relaxed)
}

/// Records the allocator totals of a sample.
pub(crate) fn record_gpu_totals(totals: GpuTotals) {
    *GPU_TOTALS.lock().unwrap_or_else(PoisonError::into_inner) = Some(totals);
}

/// Records the terrain arena's tracked buffer capacity.
pub(crate) fn record_chunk_arena(bytes: u64) {
    CHUNK_ARENA.store(bytes.min(UNKNOWN - 1), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_totals_are_read_back_as_one_sample() {
        let sample = GpuTotals {
            allocated: 300 << 20,
            reserved: u64::MAX,
        };
        record_gpu_totals(sample);
        assert_eq!(gpu_totals(), Some(sample));
    }

    #[test]
    fn the_arena_capacity_never_reads_as_unknown() {
        record_chunk_arena(u64::MAX);
        assert_eq!(chunk_arena_bytes(), Some(u64::MAX - 1));
    }
}
