//! Shared, sorted source metadata for full-resolution artwork requests.

use std::{collections::BTreeMap, ops::Deref, sync::Arc};

/// Sorted keys and their shared encoded source bytes.
type SourceEntries = Vec<(String, Arc<[u8]>)>;

/// An immutable view of oversized texture keys and their original encoded bytes.
/// The atlas keeps at most `MAX_OVERSIZED` entries. Readers share the current vector;
/// a source edit detaches it without copying pixels or retaining previous snapshots.
/// The second field orders entry indices most recently drawn first, as of the last edit.
#[derive(Clone, Default)]
pub struct OversizedTextures(Option<Arc<SourceEntries>>, Option<Arc<[usize]>>);

impl OversizedTextures {
    /// Whether both views retain the same metadata, without walking their entries.
    pub fn shares_storage(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// Compares keys and byte ownership, inspecting entries only for different snapshots.
    pub fn same(&self, other: &Self) -> bool {
        self.shares_storage(other)
            || (self.len() == other.len()
                && self
                    .iter()
                    .zip(other.iter())
                    .all(|(a, b)| a.0 == b.0 && Arc::ptr_eq(&a.1, &b.1)))
                && self.priority().eq(other.priority())
    }

    /// Entry indices most recently drawn first; key order when no edit recorded one.
    fn priority(&self) -> impl Iterator<Item = usize> + '_ {
        let order = self.1.as_deref().filter(|order| order.len() == self.len());
        (0..self.len()).map(move |index| order.map_or(index, |order| order[index]))
    }

    /// Entries most recently drawn first, so a bounded consumer such as the art pages keeps
    /// the textures on screen when it cannot hold them all.
    pub fn by_priority(&self) -> impl Iterator<Item = &(String, Arc<[u8]>)> {
        let entries: &[(String, Arc<[u8]>)] = self;
        self.priority().map(move |index| &entries[index])
    }

    /// Publishes a changed source in key order, preserving identical byte ownership on retries;
    /// returns whether the entries changed.
    pub(super) fn insert(&mut self, key: &str, bytes: &Arc<[u8]>) -> bool {
        let index = self.binary_search_by(|entry| entry.0.as_str().cmp(key));
        match index {
            Ok(index) if Arc::ptr_eq(&self[index].1, bytes) => return false,
            Err(_) if self.len() >= super::MAX_OVERSIZED => return false,
            _ => {}
        }
        self.1 = None;
        let entries = Arc::make_mut(self.0.get_or_insert_with(|| Arc::new(Vec::new())));
        match index {
            Ok(index) => entries[index].1 = Arc::clone(bytes),
            Err(index) => {
                entries.reserve_exact(1);
                entries.insert(index, (key.to_owned(), Arc::clone(bytes)));
            }
        }
        true
    }
}

impl OversizedTextures {
    /// Drops `key`'s entry, detaching the shared vector like an edit.
    fn remove(&mut self, key: &str) {
        let Ok(index) = self.binary_search_by(|entry| entry.0.as_str().cmp(key)) else {
            return;
        };
        if let Some(entries) = self.0.as_mut() {
            Arc::make_mut(entries).remove(index);
        }
        self.1 = None;
    }
}

/// The atlas's oversized textures with the atlas clock each was last drawn on. When full,
/// a new texture replaces the one drawn longest ago, never one drawn this frame, so a
/// screen that scrolls past many large images keeps offering the visible ones.
#[derive(Clone, Default)]
pub(super) struct OversizedSources {
    view: OversizedTextures,
    drawn: BTreeMap<String, u64>,
}

impl OversizedSources {
    /// The current entries, shared with art-page consumers.
    pub(super) fn view(&self) -> OversizedTextures {
        self.view.clone()
    }

    /// Records that a frame at `clock` draws `key`, when it is retained.
    pub(super) fn touch(&mut self, key: &str, clock: u64) {
        if let Some(drawn) = self.drawn.get_mut(key) {
            *drawn = clock;
        }
    }

    /// Retains `key`'s source for a frame at `clock`, replacing the least recently drawn
    /// entry from an earlier frame when full; skipped when this frame draws every entry.
    pub(super) fn insert(&mut self, key: &str, bytes: &Arc<[u8]>, clock: u64) {
        if self.view.len() >= super::MAX_OVERSIZED && !self.drawn.contains_key(key) {
            let Some(stale) = self
                .drawn
                .iter()
                .filter(|(_, drawn)| **drawn < clock)
                .min_by_key(|(_, drawn)| **drawn)
                .map(|(stale, _)| stale.clone())
            else {
                return;
            };
            self.view.remove(&stale);
            self.drawn.remove(&stale);
        }
        if !self.view.insert(key, bytes) {
            return;
        }
        self.drawn.insert(key.to_owned(), clock);
        let mut order: Vec<usize> = (0..self.view.len()).collect();
        // Stable: entries drawn on the same frame stay in key order.
        order.sort_by_key(|&index| {
            std::cmp::Reverse(self.drawn.get(&self.view[index].0).copied().unwrap_or(0))
        });
        self.view.1 = Some(order.into());
    }
}

impl Deref for OversizedTextures {
    type Target = [(String, Arc<[u8]>)];

    /// Borrows the sorted entries without creating an empty allocation.
    fn deref(&self) -> &Self::Target {
        self.0.as_deref().map_or(&[], Vec::as_slice)
    }
}

// Fixtures may exceed the atlas's source bound to exercise downstream admission limits.
#[cfg(any(test, feature = "test-support"))]
impl FromIterator<(String, Arc<[u8]>)> for OversizedTextures {
    /// Builds a sorted fixture with one entry per key, retaining the last supplied bytes.
    fn from_iter<T: IntoIterator<Item = (String, Arc<[u8]>)>>(entries: T) -> Self {
        let entries: std::collections::BTreeMap<_, _> = entries.into_iter().collect();
        if entries.is_empty() {
            Self::default()
        } else {
            Self(Some(Arc::new(entries.into_iter().collect())), None)
        }
    }
}

#[cfg(test)]
mod tests;
