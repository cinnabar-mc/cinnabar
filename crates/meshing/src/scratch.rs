//! Worker-owned temporary storage, independent of mesh results and runtime assets.

use crate::{chunk::opaque::CubeQuadStreams, contributors::PaletteScratch};

/// Reusable section-meshing buffers. Results never borrow these allocations.
#[derive(Default)]
pub struct MeshScratch {
    pub(crate) palette: PaletteScratch,
    pub(crate) cubes: CubeQuadStreams,
}

/// Keep at most four ordinary sections' worth of any variable-size temporary buffer.
pub(crate) const RETAIN_MULTIPLIER: usize = 4;

impl MeshScratch {
    /// Heap bytes held for later jobs, excluding allocator metadata and published results.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.palette.retained_bytes() + self.cubes.retained_bytes()
    }

    /// Clears temporary data and drops oversized allocations after a completed job.
    pub(crate) fn reset(&mut self) {
        self.palette.reset();
        self.cubes.reset();
    }
}

/// Clears a buffer without reallocating, discarding exceptional capacity above `limit`.
pub(crate) fn reset_buffer<T>(buffer: &mut Vec<T>, limit: usize) {
    if buffer.capacity() > limit {
        *buffer = Vec::new();
    } else {
        buffer.clear();
    }
}
