//! The store's content revision and the only write path to the contents it covers.

use std::sync::atomic::{AtomicU64, Ordering};

/// Source of every [`super::ChunkStore::content_revision`], shared by all stores so no two
/// contents observed in this process share a value.
static CONTENT_REVISIONS: AtomicU64 = AtomicU64::new(1);

/// A revision no store has reported before.
pub(super) fn next_content_revision() -> u64 {
    CONTENT_REVISIONS.fetch_add(1, Ordering::Relaxed)
}

/// Store contents readable through `Deref` but writable only through [`Self::edit`], which
/// retires the store's content revision, so no mutation can leave that revision stale.
#[derive(Debug, Default, Clone)]
pub(super) struct Revised<T>(pub(super) T);

impl<T> std::ops::Deref for Revised<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> Revised<T> {
    /// Retires `revision`, then opens the contents for writing.
    pub(super) fn edit<'a>(&'a mut self, revision: &mut u64) -> &'a mut T {
        *revision = next_content_revision();
        &mut self.0
    }
}
