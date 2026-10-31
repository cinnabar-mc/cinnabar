//! Reusable slot arrays for independent transactional variable copies.

use super::{Arc, Slot};
use std::sync::{Mutex, OnceLock, PoisonError};

/// Cleared spare sets bound retained memory; a cache miss always allocates valid storage.
const SPARE_SETS: usize = 4;

/// Copies share their source's cache, including when evaluation and retirement use different threads.
#[derive(Clone, Default)]
pub(super) struct Spares(OnceLock<Arc<Mutex<[Option<Storage>; SPARE_SETS]>>>);

impl std::fmt::Debug for Spares {
    /// Cache contents are not authored state and must not affect state diagnostics.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Spares")
    }
}

impl Spares {
    /// Takes cleared arrays from this snapshot family without limiting content admission.
    pub fn lease(&self) -> Storage {
        self.0
            .get_or_init(|| Arc::new(Mutex::new([const { None }; SPARE_SETS])))
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter_mut()
            .find_map(Option::take)
            .unwrap_or_default()
    }

    /// Returns retired arrays to their source family after releasing all string ownership.
    pub fn recycle(&self, mut storage: Storage) {
        let Some(spares) = self.0.get() else {
            return;
        };
        if storage.values.capacity() == 0 && storage.temps.capacity() == 0 {
            return;
        }
        storage.clear();
        storage.writes.clear();
        let mut spares = spares.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(spare) = spares.iter_mut().find(|spare| spare.is_none()) {
            *spare = Some(storage);
        }
    }
}

/// Owned arrays have no destructor and never retain their cache, so recycling cannot form a cycle.
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
