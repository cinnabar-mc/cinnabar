//! Shared, sorted source metadata for full-resolution artwork requests, and the registry
//! that decides which oversized textures the art pages are offered.

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Deref,
    sync::Arc,
};

/// Oversized textures offered to the art pages at once: enough for a screenful of large
/// server-menu icons and their backdrops.
pub(super) const MAX_OVERSIZED: usize = 32;

/// Sorted keys and their shared encoded source bytes.
type SourceEntries = Vec<(String, Arc<[u8]>)>;

/// An immutable view of oversized texture keys and their original encoded bytes.
/// The atlas keeps at most `MAX_OVERSIZED` entries. Readers share the current vector;
/// a source edit detaches it without copying pixels or retaining previous snapshots.
#[derive(Clone, Default)]
pub struct OversizedTextures(Option<Arc<SourceEntries>>);

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
    }

    /// Publishes a changed source in key order, preserving identical byte ownership on retries.
    pub(super) fn insert(&mut self, key: &str, bytes: &Arc<[u8]>) {
        let index = self.binary_search_by(|entry| entry.0.as_str().cmp(key));
        match index {
            Ok(index) if Arc::ptr_eq(&self[index].1, bytes) => return,
            Err(_) if self.len() >= MAX_OVERSIZED => return,
            _ => {}
        }
        let entries = Arc::make_mut(self.0.get_or_insert_with(|| Arc::new(Vec::new())));
        match index {
            Ok(index) => entries[index].1 = Arc::clone(bytes),
            Err(index) => {
                entries.reserve_exact(1);
                entries.insert(index, (key.to_owned(), Arc::clone(bytes)));
            }
        }
    }
}

impl OversizedTextures {
    /// Drops `key`'s entry, detaching the shared vector like any other edit.
    fn remove(&mut self, key: &str) {
        if let Ok(index) = self.binary_search_by(|entry| entry.0.as_str().cmp(key))
            && let Some(entries) = self.0.as_mut()
        {
            Arc::make_mut(entries).remove(index);
        }
    }
}

/// The oversized textures an ended frame drew.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct OversizedFrame {
    /// The drawn keys, when they differ from the previous frame's.
    pub changed: Option<BTreeSet<String>>,
    /// Some drawn key has no full-resolution copy.
    pub uncovered: bool,
}

/// Which oversized textures the art pages are offered. A texture drawn while the registry is
/// full replaces one no frame has drawn since the last [`Registry::end_frame`], so a screen
/// that scrolls through more large images than fit keeps offering the ones on screen.
#[derive(Default)]
pub(super) struct Registry {
    offered: OversizedTextures,
    /// The frame each offered key was last drawn on.
    drawn: BTreeMap<String, u64>,
    /// Resident textures whose page copy is downscaled, with their source bytes, so a frame
    /// that draws them without repacking still offers them.
    fitted: BTreeMap<String, Arc<[u8]>>,
    frame: u64,
}

impl Registry {
    /// The offered textures, shared with the art pages.
    pub(super) fn offered(&self) -> OversizedTextures {
        self.offered.clone()
    }

    /// Records that the current frame draws `key` at a fitted size, offering its source.
    pub(super) fn draw(&mut self, key: &str, bytes: &Arc<[u8]>) {
        if !self.drawn.contains_key(key) && self.offered.len() >= MAX_OVERSIZED {
            let frame = self.frame;
            let Some(stale) = self
                .drawn
                .iter()
                .filter(|(_, drawn)| **drawn < frame)
                .min_by_key(|(key, drawn)| (**drawn, key.as_str()))
                .map(|(key, _)| key.clone())
            else {
                return;
            };
            self.offered.remove(&stale);
            self.drawn.remove(&stale);
        }
        self.offered.insert(key, bytes);
        match self.drawn.get_mut(key) {
            Some(drawn) => *drawn = self.frame,
            None => {
                self.drawn.insert(key.to_owned(), self.frame);
            }
        }
    }

    /// Records that a resident texture is drawn this frame, offering it when it was fitted.
    pub(super) fn draw_resident(&mut self, key: &str) {
        if let Some(bytes) = self.fitted.get(key).cloned() {
            self.draw(key, &bytes);
        }
    }

    /// Records that the current frame draws an offered `key` from its full-resolution copy
    /// without placing it on a page.
    pub(super) fn draw_art(&mut self, key: &str) {
        if let Some(drawn) = self.drawn.get_mut(key) {
            *drawn = self.frame;
        }
    }

    /// Remembers a texture packed downscaled onto a page.
    pub(super) fn fitted(&mut self, key: &str, bytes: &Arc<[u8]>) {
        self.fitted.insert(key.to_owned(), Arc::clone(bytes));
    }

    /// Forgets a texture that left its page.
    pub(super) fn evicted(&mut self, key: &str) {
        self.fitted.remove(key);
    }

    /// Ends the frame, comparing the offered keys it drew with the `previous` frame's;
    /// `has_art` tells whether a key has a full-resolution copy. Allocates only when the drawn
    /// set changed.
    pub(super) fn end_frame(
        &mut self,
        previous: &BTreeSet<String>,
        has_art: impl Fn(&str) -> bool,
    ) -> OversizedFrame {
        let frame = self.frame;
        self.frame += 1;
        let drawn = self
            .drawn
            .iter()
            .filter(move |(_, drawn)| **drawn == frame)
            .map(|(key, _)| key);
        OversizedFrame {
            changed: (!drawn.clone().eq(previous.iter())).then(|| drawn.clone().cloned().collect()),
            uncovered: drawn.clone().any(|key| !has_art(key)),
        }
    }
}

impl super::ServerAtlas {
    /// Drawn textures a page had to shrink, by key, with their source bytes.
    pub fn oversized(&self) -> OversizedTextures {
        self.oversized.offered()
    }

    /// Records that the current frame draws `key` from its full-resolution copy instead of
    /// requiring it, so the art pages keep it among the textures on screen.
    pub fn draw_art(&mut self, key: &str) {
        self.oversized.draw_art(key);
    }

    /// Ends a presented frame, comparing the oversized textures it drew with the `previous`
    /// frame's; `has_art` tells whether a key has a full-resolution copy.
    pub fn end_oversized_frame(
        &mut self,
        previous: &BTreeSet<String>,
        has_art: impl Fn(&str) -> bool,
    ) -> OversizedFrame {
        self.oversized.end_frame(previous, has_art)
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
            Self(Some(Arc::new(entries.into_iter().collect())))
        }
    }
}

#[cfg(test)]
mod tests;
