//! Account-scoped marketplace delivery, signed receipts and fail-closed runtime authority.

mod guard;
mod storage;
#[cfg(test)]
pub(crate) mod tests;
mod verify;
mod worker;
pub use guard::{Guard, managed};
pub use storage::Downloaded;
pub(crate) use storage::{finish_migration, retire_receipt};
pub use verify::{Keys, Manifest, SignedRelease, SignedRevocations};

use crossbeam_channel::{Receiver, Sender, bounded};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

/// Account-bound delivery and update selection; the library owns consent and replacement.
pub struct Service {
    requests: Sender<()>,
    context: Arc<Mutex<Option<PathBuf>>>,
    downloads: Receiver<Downloaded>,
    messages: Receiver<String>,
    generation: Arc<AtomicU64>,
    configured: Option<(PathBuf, u64)>,
    guard: Arc<Guard>,
    policy: Arc<worker::updates::Policy>,
}

impl Service {
    /// Starts delivery with shipped trust roots and the existing host's source ceiling.
    pub fn start<B: crate::mod_library::port::Backend>(
        mods: PathBuf,
        guard: Arc<Guard>,
    ) -> std::io::Result<Self> {
        let limit = B::MAX_SOURCE_BYTES;
        let policy = Arc::new(worker::updates::Policy::default());
        let preferences = policy.clone();
        let (requests, incoming) = bounded(1);
        let (outgoing, downloads) = bounded(1);
        let (events, messages) = bounded(8);
        let generation = Arc::new(AtomicU64::new(0));
        let context = Arc::new(Mutex::new(None));
        let endpoint = context.clone();
        let current = generation.clone();
        let authority = guard.clone();
        std::thread::Builder::new()
            .name("marketplace".into())
            .spawn(move || {
                if let Err(error) = worker::run(
                    mods,
                    limit,
                    authority,
                    current,
                    endpoint,
                    incoming,
                    outgoing,
                    &events,
                    worker::updates::Host {
                        revision: B::HOST_REVISION,
                        api_revisions_json: B::MOD_API_REVISIONS_JSON,
                    },
                    preferences,
                ) {
                    let _ = events.try_send(format!("Marketplace unavailable: {error:#}"));
                }
            })?;
        Ok(Self {
            requests,
            context,
            downloads,
            messages,
            generation,
            configured: None,
            guard,
            policy,
        })
    }

    /// Cancels old-account downloads at the frame boundary and replaces the control endpoint.
    pub fn configure(&mut self, context: Option<(PathBuf, u64)>) {
        if self.configured == context {
            return;
        }
        let mut endpoint = self.context.lock().unwrap_or_else(|e| e.into_inner());
        let epoch = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.guard.configure(epoch);
        self.policy.clear();
        self.configured = context;
        *endpoint = self.configured.as_ref().map(|(path, _)| path.clone());
        let _ = self.requests.try_send(());
    }

    /// Applies update preferences and manual requests through the worker's publication fence.
    pub fn updates(&self, checks: bool, automatic: bool, manual: impl IntoIterator<Item = u64>) {
        if self.policy.configure(checks, automatic, manual) {
            let _ = self.requests.try_send(());
        }
    }

    /// Returns verified offers for the existing Mods and Home update states.
    pub fn offers(&self) -> Vec<launcher::mods::AvailableUpdate> {
        self.policy.offers()
    }

    /// Drains complete verified downloads for off-frame component compatibility validation.
    pub fn downloads(&self) -> Vec<Downloaded> {
        self.downloads.try_iter().collect()
    }

    /// Returns the latest bounded delivery status for the existing Mods tab.
    pub fn message(&self) -> Option<String> {
        self.messages.try_iter().last()
    }
}

/// Reads the wall clock only at the I/O boundary; verification accepts explicit time in tests.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
