//! Reusable slot arrays for independent transactional variable copies.

use super::{Arc, Slot};
use std::cell::RefCell;

/// Cleared spare sets bound retained memory; a cache miss always allocates valid storage.
const SPARE_SETS: usize = 4;

thread_local! {
    static SPARES: RefCell<Vec<Storage>> = const { RefCell::new(Vec::new()) };
}

/// Owned arrays have no destructor, so returning them cannot recurse during thread teardown.
#[derive(Debug, Default)]
pub(super) struct Storage {
    pub values: Vec<Slot>,
    pub temps: Vec<Slot>,
    /// Every slot assigned since clearing, including slots subsequently unassigned.
    assigned: Vec<u64>,
    /// Temporary slots assigned since their last clear.
    pub live_temps: Vec<usize>,
    /// Each stored string slot owns one live side-table entry.
    pub strings: Vec<Option<Arc<str>>>,
    pub free_strings: Vec<u32>,
    pub writes: Vec<u64>,
}

impl Storage {
    /// Takes an empty spare without imposing a limit on authored layouts.
    pub fn lease() -> Self {
        SPARES
            .try_with(|spares| {
                spares
                    .try_borrow_mut()
                    .ok()
                    .and_then(|mut spares| spares.pop())
            })
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    /// Marks one valid assignment in the combined variable and temporary slot space.
    pub fn assign(&mut self, slot: usize) {
        let word = slot / u64::BITS as usize;
        if self.assigned.len() <= word {
            self.assigned.resize(word + 1, 0);
        }
        self.assigned[word] |= 1 << (slot % u64::BITS as usize);
    }

    /// Unassigns visited slots and releases string ownership while retaining array capacities.
    pub fn clear(&mut self) {
        for slot in indices(&self.assigned) {
            #[cfg(test)]
            super::super::selection_work::record(|work| work.reset += 1);
            if slot < self.values.len() {
                self.values[slot] = Slot::Unset;
            } else {
                self.temps[slot - self.values.len()] = Slot::Unset;
            }
        }
        self.assigned.clear();
        self.live_temps.clear();
        self.strings.clear();
        self.free_strings.clear();
    }

    /// Copies visited slots into independent arrays, clearing the old layout before resizing.
    pub fn copy_from(&mut self, source: &Self) {
        self.clear();
        self.values.resize(source.values.len(), Slot::Unset);
        self.temps.resize(source.temps.len(), Slot::Unset);
        for slot in indices(&source.assigned) {
            #[cfg(test)]
            super::super::selection_work::record(|work| work.copied += 1);
            if slot < source.values.len() {
                self.values[slot] = source.values[slot];
            } else {
                let slot = slot - source.values.len();
                self.temps[slot] = source.temps[slot];
            }
        }
        self.assigned.clone_from(&source.assigned);
        self.live_temps.clone_from(&source.live_temps);
        self.strings.clone_from(&source.strings);
        self.free_strings.clone_from(&source.free_strings);
        self.writes.clone_from(&source.writes);
    }

    /// Returns cleared arrays when there is spare capacity in this thread's cache.
    pub fn recycle(mut self) {
        if self.values.capacity() == 0 && self.temps.capacity() == 0 {
            return;
        }
        self.clear();
        self.writes.clear();
        let _ = SPARES.try_with(|spares| {
            if let Ok(mut spares) = spares.try_borrow_mut()
                && spares.len() < SPARE_SETS
            {
                spares.push(self);
            }
        });
    }
}

/// Visits set bits in slot order without allocating an index list.
fn indices(words: &[u64]) -> impl Iterator<Item = usize> + '_ {
    words.iter().enumerate().flat_map(|(word, &bits)| {
        let mut bits = bits;
        std::iter::from_fn(move || {
            (bits != 0).then(|| {
                let slot = word * u64::BITS as usize + bits.trailing_zeros() as usize;
                bits &= bits - 1;
                slot
            })
        })
    })
}
