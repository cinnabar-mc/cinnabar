//! Images the server pack lacks: the local vanilla pack's and downloaded URLs.

use super::{MAX_EXTRA, ServerAtlas, Source, source};
use crate::sources::{RemoteImage, is_remote};

impl ServerAtlas {
    /// The vanilla image or downloaded URL behind `key`. A vanilla miss is
    /// remembered; a URL still loading is asked again next time.
    pub(super) fn fallback(&self, key: &str) -> Option<Source> {
        if let Some(found) = self.extra.borrow().get(key) {
            return found.clone();
        }
        let (found, settled) = if is_remote(key) {
            match self.remote.as_ref()?.load(key) {
                RemoteImage::Ready(bytes) => (source(bytes), true),
                RemoteImage::Failed => (None, true),
                RemoteImage::Loading | RemoteImage::ReadyWithoutBody => (None, false),
            }
        } else {
            self.local_source(key)
        };
        if is_remote(key) {
            let mut loading = self.loading_remote.borrow_mut();
            if settled {
                loading.remove(key);
            } else if loading.len() < MAX_EXTRA {
                loading.insert(key.to_owned());
            }
        }
        if settled {
            let mut extra = self.extra.borrow_mut();
            // Cache admission also bounds diagnostics when misses churn.
            if found.is_none() && extra.len() < MAX_EXTRA {
                tracing::debug!(target: "server_form", key, "UI texture unresolved");
            }
            if extra.len() >= MAX_EXTRA {
                extra.pop_first();
            }
            extra.insert(key.to_owned(), found.clone());
        }
        found
    }
}
