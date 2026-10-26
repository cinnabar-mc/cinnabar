use super::super::*;
use std::sync::Weak;

/// Identities of valid prior outputs proven exactly equal during worker finalization.
/// Weak handles prevent identity reuse without keeping the old payloads alive.
#[derive(Debug)]
pub(in crate::stream) struct EqualLightPrior {
    light: Weak<SubChunkLight>,
    direct: Weak<DirectSkyMask>,
}

impl EqualLightPrior {
    /// Records identities without retaining retired light or provenance payloads.
    pub(super) fn new(light: &Arc<SubChunkLight>, direct: &StoredDirectSky) -> Self {
        Self {
            light: Arc::downgrade(light),
            direct: Arc::downgrade(&direct.mask),
        }
    }

    /// Reuses equality only while both immutable snapshots and valid provenance still match.
    pub(in crate::stream) fn matches(
        &self,
        light: Option<&Arc<SubChunkLight>>,
        direct: Option<&StoredDirectSky>,
    ) -> bool {
        light.zip(direct).is_some_and(|(light, direct)| {
            direct.light_revision == light.generation()
                && std::ptr::eq(self.light.as_ptr(), Arc::as_ptr(light))
                && std::ptr::eq(self.direct.as_ptr(), Arc::as_ptr(&direct.mask))
        })
    }
}

#[cfg(test)]
pub(in crate::stream) mod work {
    use std::cell::Cell;

    thread_local! {
        static COUNTS: Cell<[usize; 4]> = const { Cell::new([0; 4]) };
    }

    /// Counts calls and sampled cells in the worker and owner boundary predicates.
    pub(in crate::stream) fn record(index: usize) {
        COUNTS.with(|counts| {
            let mut next = counts.get();
            next[index] += 1;
            counts.set(next);
        });
    }

    /// Measures boundary work performed synchronously by one operation on this thread.
    pub(in crate::stream) fn measure<T>(run: impl FnOnce() -> T) -> (T, [usize; 4]) {
        let before = COUNTS.with(Cell::get);
        let result = run();
        let after = COUNTS.with(Cell::get);
        (
            result,
            std::array::from_fn(|index| after[index] - before[index]),
        )
    }
}
